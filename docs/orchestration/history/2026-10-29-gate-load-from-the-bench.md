# gate-load-from-the-bench — handoff 2026-10-29

- **Item:** no board row yet — the brief proposes **ORCH-N4** of ORCH-104's
  handoff; `herd-merger` allocates the real number. It is the second half of
  **D191**: that decision added the gate's *second* load reading, and this one
  takes the decision away from both of the gate's readings.
- **Agent:** herd-step
- **Branch / commit:** `gate-load-from-the-bench` / `6728e79`, plus this
  handoff as the tip; based on origin/main `09229db`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger` names
  the landing here, and fills the record of `templates/landing.md`

## The rule change, stated as one

**Old rule.** `gate.sh` step 7 chose between `scripts/expected-skips/local.txt`
(at rest) and `local-under-load.txt` (the two-name delta) by reading
`/proc/loadavg` at **step 0** and again at the **top of step 7**, and applied
the delta — the whole of it — when **either** figure was over
`HERD_GATE_LOAD_NOTE` (8). The verdict named the reading that bit:
`skips=under-load(load1-end=14.99>8)`.

**New rule.** The delta is applied **per skip**, and a skip is excused only
when **both** hold:

- **by name** — the skip's assertion name is on `local-under-load.txt`
  (today exactly `flood-fifty` and `flood-two-fifty`); and
- **by reason** — that skip's own `HERD-SKIPPED` line names the host's
  one-minute load **and** this bench's load ceiling, and the figure it gives
  is **above that named ceiling and above `HERD_GATE_LOAD_NOTE`**.

Anything else is RED. The gate still reads the host twice and still refuses to
run above `HERD_GATE_LOAD_MAX`; those two figures are now **information** — in
the log, in the verdict and in the receipt — and decide nothing. D187 is
untouched: the comparison is still the **list** and never the count, and the
excuse is a per-name exception inside that comparison, not a count allowance.

**What the old rule could not catch.** A burst that begins **and** ends inside
the suite is invisible to both of the gate's samples. Measured twice on
2026-10-29 by two agents on branches that cannot move a bench's reading: the
host went 1.68 → 8.54 → 11.60 → 10.12 → 3.30, the benches read 10.12 as they
ran and correctly declined, and the gate's two samples were 1.70 and 3.30 —
so it applied the at-rest list and called two correct skips unexpected. A red
gate on a clean tree, 190-240 s of suite each time, and an invitation to bless
a flood name into `local.txt`, which is the one shortcut nobody should take.

**What it can now catch that it could not.** The old rule was wrong in the
other direction too, and nobody had noticed: a **busy host excused every delta
name whatever its reason**. A `flood-fifty` that skipped because the fake hub
could not bind a port read as **GREEN** on any busy host — an assertion nobody
checked, reported as expected, which is exactly what D27 forbids. Driven for
real, base against branch, same stub tree, same inputs:

```
BASE  host busy throughout, flood-fifty skipped "the fake hub could not bind a port"
      gate: GREEN ... skips=under-load(load1-start=12.00>8,load1-end=12.00>8)
FIX   same inputs
      gate: RED ... failed=7:unexpected-skip skips=at-rest(host-busy:...)
```

**What it can no longer catch.** Nothing that was ever *caught*, but two
things change shape and a reviewer should know both:

1. The gate no longer has an opinion of its own about the load. If a flood
   bench ever declines for load **without printing the figure and the
   ceiling** — a wording change in `fake_hub_flood.rs`, say — the gate goes
   RED where the old rule would have waved it through on a busy host. That is
   deliberate: the skip line is the only evidence, so a bench that stops
   giving evidence must stop being excused. It is also a false red waiting to
   happen if that wording is ever edited without this gate in mind, and the
   two phrasings are pinned by name in `check-orchestration.sh` so the edit
   fails loudly rather than silently.
2. A flood bench that declines **for load** while the host is genuinely quiet
   is now RED, where the old rule was also RED. Unchanged, but now it is red
   *for a stated reason* — the figure contradicts the ceiling on the same
   line — and the gate quotes the line.

I do **not** think the new rule is worse than the one it replaces. The old one
answered a question the gate cannot observe (what was the load while an
assertion was being skipped?) with a measurement taken at two moments it
chose; the new one reads the answer off the witness. It is strictly narrower
in what it excuses and strictly wider in what it refuses.

## Done

### 1. `scripts/gate.sh` step 7: the excuse comes from the bench, doubly scoped

`allowed` is now the at-rest list alone. `local-under-load.txt` is read for
its **names** into `load-delta-<sha12>.txt`, and each observed skip that is not
on the at-rest list is asked two questions in order:

- is this name on the delta list? If not: **red**, and the log says so by
  name.
- does **every** observed line for that name carry the evidence? The new
  `load_excuse()` requires the reason to match `over this bench's ceiling of
  <C>` and one of the bench's two phrasings — `one-minute load is Some(<F>)`
  (at the flood's start) or `one-minute load reached <F> mid-flood` — and
  requires `F > C` **and** `F > $HERD_GATE_LOAD_NOTE`. If not: **red**, and
  the log quotes the line it could not excuse.

The log now also says, before any of that, which load-gated benches **ran**
and which **declined**, so a bench that ran and one that was excused never
read the same.

### 2. The host's two readings are information

Step 0 and the top of step 7 still read `/proc/loadavg` (`read_load1`, the
`HERD_GATE_LOADAVG` seam is untouched) and the gate still refuses above
`HERD_GATE_LOAD_MAX`. `under_load`/`load_trigger` became
`host_busy`/`host_trigger`, used only for the verdict and the receipt. Step
0's message no longer claims the list "applies"; it says the decision is
step 7's and names where it comes from.

### 3. The verdict still answers "why this list" on its own

```
gate: GREEN … load1=<start> load1-end=<end> skips=under-load(flood-fifty=10.67>8,flood-two-fifty=10.12>8)
gate: GREEN … load1=<start> load1-end=<end> skips=at-rest
gate: GREEN … load1=<start> load1-end=<end> skips=at-rest(host-busy:load1-start=12.00>8,load1-end=12.00>8,no-bench-declined-for-load)
```

`load1-end` is now on the verdict line too, so both of the gate's own figures
are in the one line a reader reads. The receipt gains
`load-excused-skips: <names and figures, or `none`>`.

### 4. `--bless` is untouched, and no flood name is on `local.txt`

`--bless` still writes only `$HERD_GATE_EXPECTED`, still writes **no**
receipt, and still ends `gate: BLESSED … NOT A GATE RUN`. The four
`check-orchestration.sh` bless cases pass unchanged.
`scripts/expected-skips/local.txt` is byte-identical to `origin/main`'s.

### 5. Prose

`scripts/expected-skips/README.md`'s `local-under-load.txt` row and
`docs/status.md`'s gate paragraph stated the old rule and now state the new
one. `docs/status.md` stays at exactly 90 lines (its limit) — the replacement
is line-for-line.

## Not done, and why

- **No board row and no `docs/decisions.md` entry.** The brief forbids writing
  to either; the proposed row and the proposed `D-NEXT` text are below, for
  `herd-merger`.
- **`--bless` can still write a flood name into `local.txt`.** A blessing run
  taken while the host is busy adopts all fourteen names, and an at-rest run
  would then be green with a flood bench silently skipped. That hazard is
  unchanged by this branch and is documented in `local-under-load.txt`'s own
  header. Changing `--bless` was out of scope — the brief says not to weaken
  it — and the honest fix is probably for `--bless` to refuse to write a name
  that is on the delta list, which is a decision and not a patch.
- **`herd-hub/tests/fake_hub_flood.rs` untouched**, `LOAD_CEILING` included,
  and no test's skip behaviour changed.
- **No second host.** A green gate is evidence from one host, as the gate's own
  header says.

## Gates

**`./scripts/check-orchestration.sh` — the baseline, measured here and not
taken from the brief.** On `origin/main` `09229db` in this worktree, before
any edit:

```
check-orchestration: 256 passed, 0 failed
```

**256, not 257.** The brief said to measure it rather than trust a figure, and
256 is what this tree gives. After the change, on the committed tree:

```
check-orchestration: 269 passed, 0 failed
```

**+13, and which cases.** Six assertions that encoded the old rule are gone:
*the load-conditional list allows a load skip above the threshold*, *and below
the threshold the same skip is still RED*, *a quiet start and a busy finish
still gets the load list*, *the verdict names the reading that allowed the
extra skips*, *a quiet run throughout is still RED on that skip*, *an at-rest
run says which list it applied too* (the last name is reused by a new case).
Nineteen are new: the burst inside the suite is GREEN; the verdict names the
bench, its figure and its own ceiling; it was green with both of the gate's
**own** readings quiet; a figure under the gate's threshold excuses nothing;
the mid-flood wording is read as evidence too; a load refusal under a name the
delta list lacks is RED; that name is named in the red block; a delta name
skipping for another reason is RED; the line it could not excuse is quoted; and
RED on a busy host, where two readings called it GREEN; load claimed with a
figure under the ceiling it names is RED; the same contradiction in the
mid-flood wording is RED; a skip that merely claims load, with no figure, is
RED; over the gate's threshold and under the line's own ceiling is RED; above
that same named ceiling the skip is excused; with no delta list a load refusal
is RED and says which file; an at-rest run says which list it applied; at rest
the load-gated benches are reported as having run; a busy host with no bench
declining gets the at-rest list. 256 − 6 + 19 = 269.

**Each new case seen red, two ways.** Running the **new** case set against the
**base** `gate.sh`: `259 passed, 8 failed`, the eight being *the refusing
bench's own reading must excuse its skip*, *the verdict must name the reading
that justified the list*, *the mid-flood wording is read as evidence too*, *the
unexcusable skip line must be quoted*, *and RED on a busy host, where two
readings called it GREEN*, *with no delta list, a load refusal is RED and says
which file*, *a bench that ran must be said to have run*, *the
busy-host-at-rest verdict must say both*. The other eleven are red under the
base too (the base, being at rest in those cases, applied no delta), so
they pin the rule forward rather than discriminate against the old one — and
that was checked by mutating the **new** gate, one part of the rule at a time:

| mutation of the new `gate.sh` | result | caught by |
|---|---|---|
| drop the BY NAME scope (`if false; then`) | 266 passed, **3 failed** | *a load refusal under a name the delta list lacks is RED*; *the unexpected name must be named*; *with no delta list, a load refusal is RED and says which file* |
| drop the BY REASON scope (`load_excuse` always yields) | 258 passed, **11 failed** | eleven, including all three adverse-reason cases |
| drop `figure > ceiling` only | 268 passed, **1 failed** | *over the gate's threshold, under the line's own ceiling: RED* |
| drop `figure > $load_note` only | 268 passed, **1 failed** | *a figure under the gate's own threshold excuses nothing* |

Every part of the rule is caught by at least one case. The two comparisons are
redundant while the bench's ceiling and `HERD_GATE_LOAD_NOTE` are both 8, which
is why there is a case whose line names a ceiling of 20.

**`./scripts/check-docs.sh`** on the committed tree, before this handoff:

```
docs ok
```

with its one standing note, unchanged from `main`: *note: D170 is a gap in
docs/decisions.md (held by an open PR, or lost); gaps stay (D148)*.

**The four Rust gates ran**, inside `./scripts/gate.sh` — this is a code change
(`scripts/`), so they were not skipped. See the real runs below; every one of
them ran steps 3, 4, 5 and 6 (`nix flake check`, `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets -- -D warnings`,
`nix develop --command ./scripts/test.sh`) with
`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/gate-load`.

### The real runs, on this host, with a real burst

`./scripts/bench-preflight.sh` was run before each and said *ok, safe to
start* each time. The burst is 14 `nice -n 19` spinners, started when the
suite's own output shows `Running tests/big_log.rs` (or `tests/admission.rs`
for runs A and B) and reaped **by PID** after its length; `fake_hub_flood`
runs about 20-45 s later, so the burst begins **and** ends inside the suite.

| run | tree | burst | gate's load1 (start → after suite) | the benches read | verdict |
|---|---|---|---|---|---|
| **A** | base `09229db` | 63→163 s of step 6 (40→258 s) | **1.75 → 5.95**, neither above 8 | **13.34** | `gate: RED … failed=7:unexpected-skip skips=at-rest` |
| **B** | branch `6728e79` | 63→163 s of step 6 | **4.77 → 5.91** | **14.96** | step 7 **ok**, excuse applied; `gate: RED … failed=6:test-suite` — a *suite* flake, below |
| **C** | branch `6728e79` | mis-triggered: landed on steps 1-5 | 2.27 → 2.77 | — (floods **ran**) | `gate: GREEN … skips=at-rest` |
| **D** | branch `6728e79` | 89→159 s of step 6 (32→243 s) | **1.80 → 6.11** | **11.53** | `gate: GREEN … skips=under-load(flood-fifty=11.53>8,flood-two-fifty=11.53>8)` |
| **E** | branch `6728e79` | 89→159 s of step 6 (32→243 s) | **4.83 → 5.05** | **9.71** | `gate: GREEN … skips=under-load(flood-fifty=9.71>8,flood-two-fifty=9.71>8)` |

**1. The defect, reproduced on the base (run A).** Steps 0 to 6 all ok, suite
218 s, `test suite: PASSED`, 14 skips. Only step 7 red:

```
  the at-rest list applies on its own: load1 1.75 at the start and
  5.95 after the suite, neither above 8.
  SKIPS THAT ARE NOT ON THE EXPECTED LIST — the gate is red on these,
  because an assertion nobody expected to be skipped was not checked:
    + flood-fifty
    + flood-two-fifty
gate: RED sha=09229db9e68e3cb49abebd18617486478c8849e3 failed=7:unexpected-skip skips=at-rest
```

against the benches' own lines in that same run's suite output:

```
HERD-SKIPPED: flood-fifty: the host's one-minute load is Some(13.34), over this bench's ceiling of 8; the host is shared with a production system
HERD-SKIPPED: flood-two-fifty: the host's one-minute load is Some(13.34), over this bench's ceiling of 8; the host is shared with a production system
```

The host went 1.75 → (burst) → **15.94** peak → 6.38; the gate sampled 1.75
and 5.95; the benches read 13.34. A clean tree, red for a reason not in the
diff, 248 s spent.

**2. The same condition GREEN after the change, and green *because the excuse
applied*.** Runs **B**, **D** and **E** all took the formerly-red path: in
each, the two flood benches declined for load, the gate's own two readings
were **both under 8**, and step 7 was **ok with the excuse printed by name,
figure and ceiling** — the exact state that was RED on the base. Run C did
**not** take that path (the burst mis-landed and the benches ran), and is
counted as the honest at-rest run instead, not as a pass of this fix. So:
**three runs on the formerly-red path (B, D, E), three times step 7 ok**,
two of them fully `gate: GREEN` and the third red only on the suite flake
of finding 1. The verdict says why each time. Run D, in full:

```
  load-gated benches that DECLINED, and must now say why:
    declined flood-fifty
    declined flood-two-fifty
  each skip that is not on the at-rest list, asked by name and then by
  reason whether the load-conditional delta excuses it:
    excused  flood-fifty: the bench itself read 11.53>8, the ceiling
             being the one its own skip line names, and declined
    excused  flood-two-fifty: the bench itself read 11.53>8, the ceiling
             being the one its own skip line names, and declined
  the skip list matches the expectation exactly.
step 7: the skips — ok
  load1 1.80 at the start, 6.11 after the suite
gate: GREEN sha=6728e79dfcfdbbc499267f7e286a9d91772410a5 steps=0-8:ok host=servernixos load1=1.80 load1-end=6.11 skips=under-load(flood-fifty=11.53>8,flood-two-fifty=11.53>8)
```

**4. An honest run at rest (run C).** The at-rest list applied, 12 skips, the
two load-gated benches reported as having **run**, and the verdict said so:

```
  load-gated benches that RAN, and whose assertions were made:
    ran      flood-fifty
    ran      flood-two-fifty
  the skip list matches the expectation exactly.
gate: GREEN sha=6728e79dfcfdbbc499267f7e286a9d91772410a5 steps=0-8:ok host=servernixos load1=2.27 load1-end=2.77 skips=at-rest
```

**3. The three adverse cases, driven for real.** Through a scratch repository
whose steps are stubs and whose stub suite prints the benches' real wording —
the same harness `check-orchestration.sh` uses, so the **real** `gate.sh` is
what decides. `BASE` is `origin/main`'s `gate.sh`, `FIX` is this branch's, same
inputs:

| what the stub suite printed | host | BASE | FIX |
|---|---|---|---|
| the two floods declining for load, burst inside the suite | quiet at both readings | `RED 7:unexpected-skip` | `GREEN skips=under-load(flood-fifty=10.67>8,flood-two-fifty=10.12>8)` |
| `c_nobody_expected_this`, plus one flood declining for load | quiet | `RED` | `RED 7:unexpected-skip` — the unexpected **name** is named, the flood is excused |
| `flood-fifty: the fake hub could not bind a port on this host` | quiet | `RED` | `RED 7:unexpected-skip`, the line quoted |
| the same, **host busy throughout** | 12.00 at both readings | **`GREEN`** — the old rule's false green | `RED 7:unexpected-skip` |
| `flood-fifty: … load is Some(3.30), over this bench's ceiling of 8` | quiet | `RED` | `RED 7:unexpected-skip`, the line quoted |
| `flood-two-fifty: … load reached 7.99 mid-flood, over … ceiling of 8` | quiet | `RED` | `RED 7:unexpected-skip`, the line quoted |
| `flood-fifty: the host's one-minute load is too high for this bench` | quiet | `RED` | `RED 7:unexpected-skip`, the line quoted |

All seven are also `check-orchestration.sh` cases, so they run on every gate
from now on.

### Skips (D27)

Every complete real run on this host skipped the **twelve** names of
`scripts/expected-skips/local.txt` — all of them the same cgroup v2 delegation
refusal — and the runs where the benches declined skipped **fourteen**: those
twelve plus `flood-fifty` and `flood-two-fifty`. Neither figure is the signal;
the names are. **No skip was counted as a pass anywhere**: the fourteen-name
runs print each flood's line with its load figure, the gate excuses those two
by name and reason and says so, and the twelve-name runs say the two benches
**ran**. Every run quoted above printed `test suite: PASSED` except run B,
whose suite is reported **red** below — D179: for run B the skip list is a
floor and not a count, and I have not treated its fourteen as an expectation.

### The last run, named and not quoted

`./scripts/gate.sh` on the committed tree with this handoff as its tip,
with `CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/gate-load`, after
`./scripts/bench-preflight.sh`. Its output goes to the receipt and log under
`<worktree>/target/herd-gate/`, and the verdict line verbatim is in my report
to the orchestrator. It is the last thing run, so it cannot be in this file.

## Risks and findings

1. **A red suite under load, reported red, and not mine (run B).**
   `herd-hub/tests/announced_job_ceiling.rs:1179`,
   `a_job_refused_past_the_bound_ends_and_names_it`, failed at load1 ≈ 11-15:
   `7 passed; 1 failed; 1 ignored`, suite exit 101, so
   `gate: RED … failed=6:test-suite`. The refusal it got named
   `"holding":1` and `"refusals":1` where it expected the runner to be holding
   more, and the log around it shows the runner's control link reset and
   re-dialled eight or more times under the burst. **I changed no Rust and no
   test**, and the same burst on the base (run A) left step 6 green, so this is
   a load-sensitive flake and not a regression of this branch. It is the test
   `ORCH-119`/branch `orch-119-announced-ceiling` is about, and whoever owns
   that row should see this sample. One sample — a lead, not a diagnosis.
2. **The gate now depends on the wording of a skip line.** `load_excuse()`
   matches `over this bench's ceiling of <C>` and one of
   `one-minute load is Some(<F>)` / `one-minute load reached <F> mid-flood`. If
   `fake_hub_flood.rs` changes that wording, the gate goes **RED** on a correct
   skip. That is the price of taking the reading from the witness, and it is
   deliberate — a bench that stops saying why must stop being excused — but it
   is a coupling that did not exist before. Mitigation in the tree: both
   phrasings are reproduced verbatim in `check-orchestration.sh`
   (`flood_start_line`, `flood_mid_line`) with a comment saying why, so an edit
   to the bench that breaks the gate breaks a case first, locally, in 20 s,
   rather than after a 240 s suite.
3. **`HERD_GATE_LOAD_NOTE` and the bench's `LOAD_CEILING` are two names for 8
   and nothing ties them together.** The excuse requires the figure to clear
   both, so if the bench's ceiling were raised above 8 the gate would still
   excuse a skip whose figure sat between the two — and if it were lowered, the
   gate would refuse one the bench correctly declined. I did **not** add a
   check that the two agree: `LOAD_CEILING` is Rust and the gate is shell, and
   the only honest link is the figure printed in the line, which is what the
   gate reads. Worth a decision if the ceiling ever moves.
4. **`docs/status.md` was at exactly its 90-line limit.** My first edit added
   four lines, `check-docs.sh` went red, and — this is the part worth writing
   down — **`check-orchestration.sh` then failed 13 cases**, none of them about
   the gate: `unmodified copy passes`, the four width cases, the three fence
   cases and so on, because those cases copy the repository and run
   `check-docs.sh` on it. A docs-length overflow therefore shows up as a dozen
   unrelated red cases. The edit is now line-for-line and the file is still 90
   lines.
5. **`docs/status.md` names `D-NEXT`.** `check-docs.sh`'s `D-NEXT` check reads
   only `docs/decisions.md` headers, so this does not trip it; the merger
   should replace it with the allocated number when it appends the entry.
6. **A RED verdict can now carry `skips=under-load(…)`.** Run B's does. That is
   correct — one skip was excused and another step failed — but a reader who
   greps for `under-load` to mean "green under load" would be wrong. The field
   says which list applied, not what the verdict is.
7. **My own driver bit me once, and it is the same class of mistake as the
   defect.** The gate's suite output file is named for the sha, so a second run
   of the same sha finds the previous run's file; my burst driver triggered on
   that stale text and put the burst on steps 1-5 instead of the suite (run C).
   Nothing in the repository is affected — the driver lives in my scratchpad —
   but it is worth knowing that `target/herd-gate/test-output-<sha12>.txt`
   survives between runs of one sha.

## Decisions needed

None blocking. Two things for the merger to allocate and record.

**Proposed board row (ORCH-N4, number allocated by `herd-merger`).**

| ORCH-N4 | **`gate.sh` decided which expected-skip list applies by sampling `/proc/loadavg` twice, and both samples miss a burst that begins *and* ends inside the suite.** Two agents' first gate runs went RED on 2026-10-29 on branches that cannot move a bench's reading: the host went 1.68 → 8.54 → 11.60 → 10.12 → 3.30, the flood benches read **10.12** and correctly declined, the gate sampled **1.70** and **3.30**, applied the at-rest list and called `flood-fifty` and `flood-two-fifty` unexpected. 190-240 s of suite lost per false red. The same rule was wrong the other way too: a **busy host excused every delta name whatever its reason**, so a flood bench that skipped because it could not bind a port read GREEN | measured twice, 2026-10-29, by two agents; reproduced deterministically on the base | herd-step | done | **Done 2026-10-29 on `gate-load-from-the-bench`.** The delta is applied **per skip**, out of the refusing bench's own skip line, and stays doubly scoped: **by name** (only `local-under-load.txt`'s names) and **by reason** (the line must name the load ceiling and give a figure above both that ceiling and `HERD_GATE_LOAD_NOTE`). The gate's two host readings stay in the verdict and the receipt as information and decide nothing. `check-orchestration.sh` 256 → **269**: nineteen cases drive the rule, six that encoded the old one are gone; 8 of the 19 go red against the base gate and four mutations of the new one are each caught. No flood name was added to `local.txt`; `--bless` is untouched | `D191` | `history/2026-10-29-gate-load-from-the-bench.md`; `scripts/gate.sh`; `scripts/check-orchestration.sh`; `scripts/expected-skips/README.md`; **D-NEXT** |

**Proposed `D-NEXT` entry** (text only — I wrote nothing to
`docs/decisions.md`;
`herd-merger` allocates the number and appends it):

> ## 2026-10-29 — D-NEXT The gate reads the load off the bench that
> refused, not off the host
>
> **Context.** `gate.sh` step 7 chose between `expected-skips/local.txt` and
> `local-under-load.txt` by sampling `/proc/loadavg` at step 0 and again after
> the suite, and applied the whole two-name delta when either figure was over
> 8 (D191). Both samples miss the case that actually happens here: a burst
> that begins **and** ends inside the suite. Measured twice on 2026-10-29, by
> two agents, on branches that cannot move a bench's reading — the host went
> 1.68 → 8.54 → 11.60 → 10.12 → 3.30, the flood benches read 10.12 as they
> ran and declined, the gate sampled 1.70 and 3.30, applied the at-rest list
> and called two correct skips unexpected. Each false red costs a whole suite
> run (190-240 s) and invites blessing a flood name into `local.txt`, which
> would leave an assertion nobody checks reported as expected. The same rule
> was wrong in the other direction and nobody had noticed: a busy host excused
> **every** delta name whatever its reason, so a flood bench that skipped
> because it could not bind a port read GREEN.
>
> **Choice.** The load-conditional list is applied **per skip**, out of the
> refusing bench's own `HERD-SKIPPED` line — the figure that bench read, at
> the moment it read it, is the only evidence about why that skip exists. The
> excuse is **doubly scoped**: the skip's name must be on
> `local-under-load.txt`, *and* its reason must name this bench's load ceiling
> and give a figure above both that ceiling and `HERD_GATE_LOAD_NOTE`. A delta
> name that skipped for any other reason, or claimed load with a figure under
> the ceiling, or claimed load with no figure, is **RED** — each is a defect
> in the bench and the gate is where it shows. D187 is untouched: the
> comparison is still the list and never the count. The gate still reads the
> host at step 0 and after the suite, and still refuses to run above
> `HERD_GATE_LOAD_MAX`; those two figures are now information, in the log, the
> verdict and the receipt, and decide nothing. The verdict says which reading
> justified the list and where it came from:
> `skips=under-load(flood-fifty=10.67>8,flood-two-fifty=10.12>8)`,
> `skips=at-rest`, or
> `skips=at-rest(host-busy:load1-start=12.00>8,load1-end=12.00>8,
> no-bench-declined-for-load)`.
>
> **Alternative rejected.** A third reading, taken mid-suite. It is the same
> mistake one sample further on: three instants the gate chose still cannot
> see a burst between them, and each extra sample makes the verdict harder to
> read while making the rule no sounder. The witness is the bench; ask it.

## Resume point

Nothing is in flight. For `herd-merger`: take `gate-load-from-the-bench` into
the batch, allocate the `D-NEXT` number, write the board row, and replace
`D-NEXT` in `docs/status.md`'s gate paragraph with it. For whoever holds
`ORCH-119`: finding 1 above is a fresh sample of
`a_job_refused_past_the_bound_ends_and_names_it` failing at load1 11-15, with
the full suite output at
`<worktree>/target/herd-gate/test-output-6728e79dfcfd.txt` — which is deleted
with the build tree, so copy it first if it is wanted.

## Processes

Everything below was started by me and ended by me, **by PID**. No `pkill`,
no `killall`, nothing killed by name. Nothing of mine is running now: a
`kill -0` sweep over every PID in this list came back empty.

- One timing probe of the suite, to learn when `fake_hub_flood` runs
  (`tests/admission.rs` at 53 s, `tests/fake_hub_flood.rs` at 99 s, suite
  213 s, 12 skips, `test suite: PASSED`): pid **1135925**, exited on its own.
- One warm-up (`cargo clippy --workspace --all-targets` then
  `cargo test --workspace --no-run`), exited 0 on its own.
- Five real gate runs, each with a load sampler and 14 `nice -n 19` spinners,
  all reaped by PID at the end of the burst:
  - run A (base): sampler **1183037**, spinners **1216324-1216337**
  - run B (branch): sampler **1250912**, spinners **1289472-1289485**
  - run C (branch): sampler **1329020**, spinners **1329032-1329045**
  - run D (branch): sampler **1407099**, spinners **1447399-1447412**
  - run E (branch): sampler **1483546**, spinners **1523561-1523574**
  Seventy spinners started, seventy reaped; none needed `SIGKILL`.
- Build tree: `CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/gate-load`, **never**
  inside a worktree and never on the root filesystem. It is removed by its
  literal path once this branch is pushed, together with the gate's own
  per-worktree receipt stores
  `<scratchpad>/wt-gate-load/target/` and
  `<scratchpad>/wt-gate-load-base/target/`, and the throwaway worktree
  `<scratchpad>/wt-gate-load-base` (a detached checkout of `origin/main`,
  created only to gate the base tree).
- Nothing was written outside my own worktree and my own scratchpad
  subdirectory. Nothing touched `/etc/nixos`, `/srv`, a container, a service
  or anything of the chatOnS project.
