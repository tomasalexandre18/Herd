# harden-the-gate — handoff 2026-10-28

- **Item:** no board row of its own — the six findings of
  `history/2026-10-28-bootstrap-the-gate.md`; it finishes **ORCH-135**'s
  last step (the skip lists are named as a precondition) and closes the
  prose half of **ORCH-131** (the advice that would restore it)
- **Agent:** herd-step
- **Branch / commit:** `harden-the-gate` — `7dd45ca` (machinery),
  `2fbb8e1` (prose), `a4c518b` (a flake the certifying run exposed), and
  this handoff as the tip; based on **`origin/bootstrap-the-gate`
  `f258e51`**, not on `origin/main` — `f258e51` carries
  `scripts/expected-skips/local.txt`, without which the gate refuses
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

### 1. A `--bless` run writes no receipt (`gate.sh` step 8)

It green-lights its own step 7 — `--bless` adopts *this* run's skips as
the expectation — and then wrote a receipt like every other run.
`--verify-receipt` reads `verdict: GREEN`, the `sha:` line and that the
file exists, and `pre-push` check 1b asks nothing else, so such a
receipt would have been the gate for a landing's second parent.

A blessing run now ends in `gate: BLESSED …`, says *"NOT A GATE RUN"*,
and writes nothing. **The fallback was considered and rejected**: a
`blessed: yes` field that `--verify-receipt` refuses leaves an object
that exists and must be disbelieved, and every reader of the store then
has to know the rule — a receipt that must be disbelieved is a worse
object than a receipt that does not exist. Writing nothing needs no
rule. The reasoning is in the file, at the branch.

Exercised for real, not only in the harness, on `2fbb8e1`, with
`HERD_GATE_EXPECTED` and `HERD_GATE_RECEIPTS` pointed at scratch paths
so that the blessing neither dirtied the tree nor touched the store:

```
gate: BLESSED sha=2fbb8e1bcf836da447da859bf134cf521fe8b3bd steps=0-8:ok host=servernixos load1=1.94 skips=at-rest
gate: NOT A GATE RUN — a blessing run green-lights its own step 7, so
      no receipt was written and this verdict authorises no push.
      Re-run without --bless to gate 2fbb8e1bcf836da447da859bf134cf521fe8b3bd.
$ ls <scratch store>            # no *.receipt at all
allowed-skips-2fbb8e1bcf83.txt  seen-skips-2fbb8e1bcf83.txt
skips-2fbb8e1bcf83.txt          test-output-2fbb8e1bcf83.txt
$ ./scripts/gate.sh --verify-receipt 2fbb8e1b… # the question pre-push asks
gate: no receipt for 2fbb8e1b… in <scratch store>
exit 1
```

The two receipts in the bootstrapping agent's store were **not**
touched; they are its evidence.

### 2. The load is read twice, and the verdict says which reading bit

`fake_hub_flood.rs` reads the one-minute load at the flood's start
(`:364`) and again mid-flood (`:511`), and skips itself on either. The
gate read it once, at step 0, so a run that began quiet and became busy
produced a flood skip that was not on the allowed list.

The second reading is taken at the top of step 7 — the first moment
after the suite, which is where the benches read theirs — and step 8
prints and records that same figure rather than reading a third one.
The load-conditional list applies when **either** reading is over
`HERD_GATE_LOAD_NOTE`, and the verdict carries a new field: `skips=at-rest`
or `skips=under-load(load1-start=9.56>8,load1-end=14.91>8)`. Step 7 also
says in words that the second list is a **delta** over the at-rest one.

Exercised for real on `2fbb8e1` with twelve `nice -n 19` spinners
started with the run and killed by PID (step 7, verbatim):

```
  observed skip lines: 14 (the count is
  not the signal; the list is)
  load1 rose from 1.41 to 14.99, above 8: the flood
  benches read the load again mid-flood and skip themselves on that
  reading too, so the load-conditional expected list applies.
  adding the load-conditional list: …/scripts/expected-skips/local-under-load.txt
  it is a DELTA over the at-rest list and never a whole list
  allowed by: load1-end=14.99>8
  the skip list matches the expectation exactly.
step 7: the skips — ok
```

Under the old single reading that run would have been RED on
`+ flood-fifty` and `+ flood-two-fifty`. **That run's verdict is RED all
the same, for an unrelated reason it uncovered — see finding 1 below and
fix 6.**

### 3. `--check-install` refuses a relative `core.hooksPath`

`pre-push:152` advised `git config core.hooksPath scripts/githooks` —
the exact value ORCH-131 removed, one setting for all 85 worktrees. The
advice is now absolute and names the **main checkout's** hooks
directory, read from `git rev-parse --git-common-dir`, so a throwaway
worktree never advises arming every other worktree with its own path.
And `--check-install` now refuses a value that does not start with `/`:
nothing could tell the two forms apart before, because
`--git-path hooks` contains `scripts/githooks` either way.

Measured in a scratch repository and one of its worktrees:

```
$ git config core.hooksPath scripts/githooks && pre-push --check-install
pre-push: core.hooksPath=scripts/githooks is RELATIVE, and the guard it arms is
…
pre-push: too (ORCH-131). Fix:
pre-push:   git config core.hooksPath <checkout>/scripts/githooks
exit 1
$ cd <worktree of it> && pre-push --check-install   # advises the CHECKOUT
pre-push:   git config core.hooksPath <checkout>/scripts/githooks
$ cd <this worktree of herd-project> && pre-push --check-install
pre-push: installed — core.hooksPath=/mnt/ssd_980/herd-project/scripts/githooks
exit 0
```

The pre-push corpora in `check-orchestration.sh` now install the
absolute form, which is what the hook requires, and the comment that
gave the relative command as the example was corrected with them.

### 4. `workflow.md` transition step 3 names six preconditions

The sixth is the skip lists, and it is the one that actually blocked
2026-10-28: without `scripts/expected-skips/local.txt` the gate does not
go red, it **refuses** — exit 3, no verdict. Both files are named, and so
is the fact that `local-under-load.txt` is a **delta** over the at-rest
list and never a full list, with the reason that distinction matters:
blessing a loaded run straight into `local.txt` would put all fourteen
names there and leave an at-rest run green with a flood bench silently
skipped.

### 5. `templates/handoff.md` no longer contradicts itself

It said the gate is "the last thing run — after this file is committed"
*and* that "the output under *Gates* is that run's". The resolution is
`herd-merger.md` step 6's, now stated for every agent: *Gates* **names**
the last run and where its output goes, the verbatim output goes to the
agent's **report**, and what is quoted in the file is from the runs made
before the commit. `workflow.md`'s *The loop* step 4 carried the same
contradiction in its own words and was corrected in the same commit —
one copy fixed and the other left wrong is the ORCH-133 family.

### 6. Written down, not fixed: a green gate does not travel

New bullet in *What a landing is*: the receipt store is per worktree
(`gate.sh:63`, under a gitignored `target/`), so *"a green gate for the
merge's second parent"* is not something a branch hands over. The merger
gates its own integration tip — its step 7 already says so — and an
agent's own green gate is evidence about its branch, never the landing's.
Nobody copies a receipt in to save a run.

### 7. Found in flight: an assertion idiom that false-fails under load

`printf '%s' "$out" | grep -q P` is unsafe under `set -o pipefail`,
which `check-orchestration.sh` sets: `grep -q` exits on its match,
`printf` dies of SIGPIPE on the rest, and the pipeline reports the
failure of a text that matched. The nine assertions this branch added
read `$out` through a here-string instead. See finding 1 for the
measurement and for the ~30 sites left alone.

## Not done, and why

- **The ~30 older `printf | grep -q` sites in
  `check-orchestration.sh`.** Mechanical but each has its own shape
  (`-qE`, `-qxF`, negated, inside `if`), and rewriting assertions this
  branch did not write is a separate task with its own gate run. It is
  finding 1 below, with a reproduction script.
- **`docs/decisions.md`, `board.md`, the branch protection, `ci.yml`** —
  all out of the brief's write paths. Proposed decision text and the
  board rows this leaves are below.
- **`--check-install` still cannot see whether the directory exists**,
  only what the config says: unchanged, asserted as a known limit by the
  case at `check-orchestration.sh:1278` and left that way.
- No pull request, no merge, no tag, nothing on `main`.

## Gates

`./scripts/gate.sh` was run **before and after** the change, since the
change is to the gate itself. Both verdicts verbatim; the only
difference is the new `skips=` field:

```
# before, on the base f258e51, untouched tree
gate: GREEN sha=f258e514fb081c13f76d671861c01120daf506db steps=0-8:ok host=servernixos load1=0.54
#   steps 0-8 ok, suite 239 s, 12 skip lines, "the skip list matches the
#   expectation exactly", load1 0.54 -> 3.53

# after, on 2fbb8e1 (machinery + prose), at rest
gate: GREEN sha=2fbb8e1bcf836da447da859bf134cf521fe8b3bd steps=0-8:ok host=servernixos load1=0.60 skips=at-rest
#   check-docs 0 s, check-orchestration 19 s, nix flake check 2 s,
#   fmt 1 s, clippy 8 s, suite 190 s, 12 skip lines, list matched
#   exactly, load1 0.60 -> 2.62
```

The verdict did **not** move: GREEN before, GREEN after, same twelve
skips, exit 0 both times.

**A third run, deliberately under load** (twelve spinners), on the same
`2fbb8e1`, is the one that certifies fix 2 and is **RED**:

```
gate: RED sha=2fbb8e1bcf836da447da859bf134cf521fe8b3bd failed=2:check-orchestration skips=under-load(load1-end=14.99>8)
```

Step 7 was **ok** in that run (14 skips, the delta applied, the list
matched exactly — quoted under fix 2 above). What failed was step 2,
`check-orchestration.sh`, on the pipefail flake of finding 1, whose own
diagnostic printed the text it called missing. That is reported red, as
it stands: nothing was edited to make it green, and the fix in `a4c518b`
addresses the nine assertions this branch owns, not the run's verdict.

`./scripts/check-orchestration.sh` on the final machinery, at rest and
then four times under a load of 9.9 to 15.7:

```
check-orchestration: 253 passed, 0 failed     (at rest)
check-orchestration: 253 passed, 0 failed     (load1 9.9-15.7, 4 runs)
```

**243 on the base, 253 here: ten new cases**, no case dropped —
`--bless` leaves no receipt, `--verify-receipt` refuses that sha, the
verdict reads BLESSED and never GREEN, a quiet start with a busy finish
gets the load list, the verdict names the triggering reading, a run
quiet throughout is still RED on that skip, an at-rest run says so, a
relative `core.hooksPath` is refused, the advice is absolute, and the
advice names the checkout.

`./scripts/check-docs.sh`: `docs ok`, exit 0, with its standing note
*"D170 is a gap in docs/decisions.md … gaps stay (D148)"*.

**The last run, named and not quoted** (`templates/handoff.md`, as this
branch now writes it): `./scripts/gate.sh` on the tip that carries this
file, receipt at `target/herd-gate/<tip sha>.receipt` in this agent's
worktree, log beside it. Its verbatim verdict is in the report to the
orchestrator, because nothing may be added to the tree that was gated.

**Skips, each one named.** Twelve at rest, all one capability (cgroup v2
delegation this host does not grant), identical to
`scripts/expected-skips/local.txt`:
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
`the_runner_says_which_memory_ceiling_it_enforces`. Under load,
**fourteen**: those twelve plus `flood-fifty` and `flood-two-fifty`, and
nothing else — exactly `local-under-load.txt` (D27).

## Risks and findings

### 1. `check-orchestration.sh` false-fails under load, and it cost a gate

**This is the finding that matters most**, because it is the same class
of defect as the brief's own fix 2: a gate that goes red on a clean tree
for a reason that is not in the diff.

`printf '%s' "$out" | grep -q P` under `set -o pipefail` reports the
pipeline as failed when `grep -q` exits on its match before `printf` has
finished writing. Measured on this host at load 8.7 with an env-shaped
input of 7.9 kB: **2 false failures in 4000** through the pipe, **0 in
4000** through a here-string. It fired twice for real on 2026-10-28:

- in the load-certifying gate run, on the at-rest verdict case this
  branch added — the run went RED at step 2 and the batch would have
  paid a full suite for it;
- in `envis` (`check-orchestration.sh:430`), run 1 of four under load:
  *"FAIL env.sh: expected HOME=…/claude/home"* with that exact line
  printed in its own dump, three lines below.

Reproduction, kept: `<scratchpad>/sigpipe4.sh` (path in *Processes*).
Nine sites are fixed; about thirty are not. **Recommendation:** one
bounded task converting the rest, either to here-strings or to one
helper per grep shape, with a load run as its evidence. It is not a new
rule and needs no decision.

### 2. Where this brief is wrong

- **Fix 3's premise about `check-orchestration.sh:1171-1176` is wrong.**
  Those lines assert a *different* known limit — that `--check-install`
  answers about the **config** and not about the **directory** (a
  `core.hooksPath` naming a missing directory still reports
  "installed"). Nothing in that file ever asserted that the two *forms*
  of the value are indistinguishable; there was **no case at all** on
  relative vs absolute, and the misleading thing was a **comment**
  (now `:1220`) that gave the relative command as its example. So there
  was no "known-limit case to flip": the directory-vs-config case is
  still true, still passes, and was left exactly as it was, while the
  relative/absolute behaviour is **three new cases**. The brief's
  conclusion — "nothing mechanical would catch the regression" — was
  right for a different reason than the one it gave.
- **"Two such receipts exist right now in that agent's store"** — not
  reachable from here to confirm: the store is `target/herd-gate/` in
  *that* agent's worktree (finding 6's point), and this branch deleted
  nothing. The defect is reproduced in the harness instead, which is
  stronger evidence than the two files anyway.
- **`gate.sh:138` / `:364` / `:511` line numbers**: `:138` was right on
  the base and is now `:165`. The two Rust line numbers are right, and
  `pre-push:152` was right.
- Everything else in the brief held, including the warning about
  treating the under-load list as a full list — it is why step 7 now
  says "DELTA" in words.

### 3. Risks this change carries

- **`HERD_GATE_LOADAVG` is a new seam.** It lets a caller feed the gate
  a load figure, which decides which skip list applies and whether the
  ceiling refuses. It is a test seam, documented as one at
  `gate.sh:68-72`, and it is strictly weaker than `HERD_GATE_RECEIPTS`,
  which has always let a caller point the gate at a store of its own. A
  reviewer who wants it gone would have to give up the "quiet start,
  busy finish" case, which is the one that reproduces the defect.
- **The second reading is taken after the suite, not mid-flood.** It is
  a proxy: a spike that rises and falls entirely inside the suite is
  still missed. Two readings cover the shape measured today (a load that
  climbs and stays); a third, read from the suite's own output, would be
  exact and is not worth a parser.
- **`skips=` in the verdict line is a format change.** One existing case
  greps `skips=|steps=0-8:ok`, so it was anticipated; anything outside
  the repository that parses the verdict (nothing known) would see a new
  trailing field.
- **The gate under load is still not a reliable red/green signal**,
  finding 1. The at-rest gate is.

## Decisions needed

1. **Does the project want the ~30 remaining `printf | grep -q` sites
   converted?** *Recommendation:* yes, as one bounded `herd-step` task
   with a load run as evidence (finding 1). *Fallback:* leave them and
   accept a few-percent false-red per gate run under load, re-reading
   the diagnostic each time. No invariant is involved either way.
2. **Proposed decision text, for `herd-merger` to allocate** (this
   branch did not touch `docs/decisions.md`):

   > **D-NEXT — a blessing run writes no receipt, and the gate reads the
   > load twice.** `./scripts/gate.sh --bless` adopts the run's own skip
   > list as the expectation, so its step 7 green-lights itself: it ends
   > in `gate: BLESSED`, authorises no push and writes **no receipt**,
   > because `--verify-receipt` reads only the verdict and the sha and a
   > receipt that must be disbelieved is worse than none. And because
   > the flood benches read the one-minute load twice — at the flood's
   > start and again mid-flood — the gate reads it at step 0 and again
   > after the suite, applies the load-conditional skip list when either
   > reading is over the threshold, and names the triggering reading in
   > its verdict (`skips=at-rest`, `skips=under-load(load1-end=14.99>8)`).
   > `scripts/githooks/pre-push --check-install` refuses a relative
   > `core.hooksPath` and advises the absolute path of the main
   > checkout's hooks directory (ORCH-131).

3. **Board rows this leaves** (not written here — `board.md` is out of
   the write paths): the five fixes as one done row pointing at this
   handoff; finding 1 as a new open row; and ORCH-135's *last step* can
   be closed, since the skip lists are now a named precondition.

## Resume point

Nothing is pending on this branch. For whoever continues:

```sh
git fetch origin harden-the-gate
# the gate on the branch tip, in a worktree of your own:
export CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/<your task>
./scripts/bench-preflight.sh && ./scripts/gate.sh
```

`herd-merger` takes this branch with the batch; its own gate run on the
integration tip is the landing's, not this one's (finding 6).

## Processes

Spinner PIDs started by this task, all `nice -n 19 yes > /dev/null`,
each killed by PID from the shell that started it, never by name:
`131112-131123` (the load-certifying gate run), `198873-198884` and
`210072-210083` (the first pipefail measurements and the three loaded
`check-orchestration.sh` runs), and `303285-303298` (the four loaded
runs after the fix). One further set of fourteen, for the env-shaped
pipefail measurement, was killed the same way but its PIDs were **not
echoed** and are therefore not quoted here; `/proc/loadavg`'s last-pid
field read `281467` just after it. `ps -u` at the end of the task lists
no `yes` process at all, which is the check that matters. No other
process was started, and nothing of the consumer's was touched.

`CARGO_TARGET_DIR` was `/mnt/ssd_980/herd/build/harden-the-gate`,
outside the worktree, removed once the branch was pushed and after every
gate run (`workflow.md`, *Concurrency and host rules*). The worktree's
own `target/herd-gate/` holds the receipts and logs of the runs quoted
above; the four gate logs and the reproduction script are kept in this
agent's scratchpad, which is session-local and not a deliverable:

```
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/
  gate-before.log  gate-after.log  gate-bless.log  gate-load.log
  co-load-1.txt (the envis false failure)  sigpipe4.sh (the measurement)
```
