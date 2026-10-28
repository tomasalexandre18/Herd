# bootstrap-the-gate — handoff 2026-10-28

- **Item:** ORCH-135, ORCH-131, and step 10 of `herd-merger.md`
- **Agent:** herd-step
- **Branch / commit:** `bootstrap-the-gate` — `7403626` (the
  at-rest list), `ede2451` (the under-load list), and this handoff as
  the tip; based on `origin/main` `7eb9585`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

### 1. ORCH-135 — the gate has an expected skip list, and it is measured

`scripts/gate.sh` refused every plain run on this workstation because
`scripts/expected-skips/local.txt` did not exist (`gate.sh:235-236`), so
no landing was possible at all. The list now exists and is this host's
own measurement.

The bootstrap, as it was actually walked, because the order is the whole
problem: `--bless` writes the list and so dirties the tree, and step 0
refuses a dirty tree, so one run can never both bless and certify a sha.

| run | load1 at step 0 | what it did | verdict |
|---|---|---|---|
| A | 0.32 | `--bless` at rest; wrote 12 lines to `local.txt` | GREEN |
| B | 9.56 | `--bless` under load into a scratch file: 14 lines, so the delta is two | GREEN |
| D | 9.04 | plain run on the committed pair, both lists read | GREEN |
| E | at rest | plain run on this branch's tip — the certifying run | see *Gates* |

Run A's suite took 240 s and ended `test suite: PASSED`, which is the
only condition under which `--bless` writes anything (D27): it refuses a
list from a suite that did not pass. All 12 skips are one capability —
cgroup v2 delegation this host does not grant (its scope is
`chatons.slice`, and a job may not create a cgroup under it) — plus the
two branches of the IO-weight and memory-ceiling reports. Not one of
them is a flood bench, which is the point of the split.

`local.txt` is committed exactly as `--bless` wrote it, header and all:
the header records `host=servernixos nproc=12 load1=0.32` and the sha it
measured on, so its provenance needs no prose.
### 2. ORCH-131 — `core.hooksPath` is absolute now

Before, measured in `/mnt/ssd_980/herd-project/.git/config`:

```
file:.git/config	scripts/githooks
```

The act, run exactly as *Concurrency and host rules* writes it, so that
the guard and the value name the same place:

```sh
test -x /mnt/ssd_980/herd-project/scripts/githooks/pre-push &&
  git -C /mnt/ssd_980/herd-project config core.hooksPath \
    /mnt/ssd_980/herd-project/scripts/githooks
```

After:

```
file:.git/config	/mnt/ssd_980/herd-project/scripts/githooks
```

It is one setting for the whole repository, as the document says:
`--git-common-dir` is `.git` and `extensions.worktreeConfig` is unset, so
there is nowhere else it could have been written.

Proved from two places, plus a third that is the reason the rule exists:

```
=== place 1: my own worktree ===
pre-push: installed — core.hooksPath=/mnt/ssd_980/herd-project/scripts/githooks
  git rev-parse --git-path hooks = /mnt/ssd_980/herd-project/scripts/githooks
=== place 2: the orchestrator's checkout (/mnt/ssd_980/herd-project) ===
pre-push: installed — core.hooksPath=/mnt/ssd_980/herd-project/scripts/githooks
  git rev-parse --git-path hooks = /mnt/ssd_980/herd-project/scripts/githooks
=== place 3 (read-only): a pre-landing worktree, 9d3d101 ===
  does it contain scripts/githooks? no
  git rev-parse --git-path hooks = /mnt/ssd_980/herd-project/scripts/githooks
pre-push: installed — core.hooksPath=/mnt/ssd_980/herd-project/scripts/githooks
```

Place 3 is `.claude/worktrees/agent-a3e03730c6434de03`, on
`m11-collection-window` at `9d3d101`, which predates the batch and has no
`scripts/githooks` of its own. Nothing was written there; the two git
commands read. What the relative value resolved to in that same worktree,
measured with `-c` so the shared config was never touched:

```
relative: scripts/githooks      exists? NO
absolute: /mnt/ssd_980/herd-project/scripts/githooks      exists? yes
```

So the brief's reading is right and mild: a relative `core.hooksPath` did
not merely fail to guard those worktrees, it pointed each of them at a
directory that does not exist, which git treats as "hooks disabled" and
supplies nothing in place of `.git/hooks`.

### 3. The orchestrator's checkout is advanced

Preconditions as commands, before anything moved: `symbolic-ref` →
`main`, `status --porcelain` → empty. Then `fetch` and `merge --ff-only`.

```
=== before ===  860c4f59a8df2cfcf389f0d6701d486f938828fa
=== after  ===  7eb95859dfe656750661a270d1b2c614616e433f
```

Fast-forward, no conflict, and the tree is still clean afterwards.
`scripts/githooks/pre-push` exists there now and is executable, which is
what item 2 depended on and why it ran second.
## Not done, and why

- **`docs/orchestration/board.md` is untouched**, as the brief asks: the
  findings below are board rows' worth of material and `herd-merger`
  writes that file.
- **No change to `gate.sh`, `pre-push`, `README.md` of
  `expected-skips/`, `workflow.md` or the handoff template**, although
  five of the findings below argue for one. The brief allows the two
  skip lists and this handoff; a change outside them is a finding, not
  an edit.
- **`workflow.md`'s transition step 3 still names five preconditions and
  not the skip list**, which ORCH-135's row asks for as its last step.
  It is prose in a file this task may not write.

## Gates

`./scripts/gate.sh` is the gate now and this was its first real use, so
all four runs are here. The lines below are verbatim, filtered to the
step verdicts and the skip comparison; `load:` lines come from my own
load helper and not from the gate. `CARGO_TARGET_DIR` was outside the
worktree throughout, and `TMPDIR` was left at the session's own
28-character path (ORCH-139).

**Run A — `--bless`, at rest, on `7eb9585`.** The bless that made a list
exist at all.

```
step 0: the tree, the sha and the host — ok
step 1: check-docs.sh (ci job: docs) — ok (1s)
step 2: check-orchestration.sh (ci job: docs) — ok (18s)
step 3: nix flake check (ci job: check) — ok (1s)
step 4: cargo fmt (ci job: check) — ok (2s)
step 5: cargo clippy (ci job: check) — ok (18s)
step 6: test suite — ok (240s)
  observed skip lines: 12 (the count is
  not the signal; the list is)
  --bless: wrote 12 lines to /mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/gate/wt/scripts/expected-skips/local.txt
  expected list: /mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/gate/wt/scripts/expected-skips/local.txt
  the skip list matches the expectation exactly.
step 7: the skips — ok
  load1 0.32 at the start, 2.36 at the end
gate: GREEN sha=7eb95859dfe656750661a270d1b2c614616e433f steps=0-8:ok host=servernixos load1=0.32
```

**Run B — `--bless` into a scratch file, under load, on `7403626`.** The
measurement the under-load list is made of. Note `expected list:` points
into my scratchpad, which is the whole reason this run's receipt is not
evidence about any tree (see *Risks and findings*).

```
load: started 11 nice-19 spinners: 3794139 3794140 3794141 3794142 3794143 3794144 3794145 3794146 3794147 3794148 3794149
load: load1 9.56 is above 9.5 after 100s
  load1 9.56 is above 8: the flood benches skip themselves
step 0: the tree, the sha and the host — ok
step 1: check-docs.sh (ci job: docs) — ok (0s)
step 2: check-orchestration.sh (ci job: docs) — ok (35s)
step 3: nix flake check (ci job: check) — ok (3s)
step 4: cargo fmt (ci job: check) — ok (3s)
step 5: cargo clippy (ci job: check) — ok (12s)
step 6: test suite — ok (257s)
  observed skip lines: 14 (the count is
  not the signal; the list is)
  --bless: wrote 14 lines to /mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/ul-blessed.txt
  expected list: /mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/ul-blessed.txt
  the skip list matches the expectation exactly.
step 7: the skips — ok
  load1 9.56 at the start, 14.91 at the end
gate: GREEN sha=7403626307a2d3ad621c35656b3742877c12fc4f steps=0-8:ok host=servernixos load1=9.56
load: the command exited 0; load1 now 14.91
load: killed spinners 3794139 3794140 3794141 3794142 3794143 3794144 3794145 3794146 3794147 3794148 3794149
load: load1 after cleanup 14.91
```

**Run D — plain, under load, on `ede2451`.** The only run that proves the
committed pair: both lists are read, and neither is short.

```
load: started 9 nice-19 spinners: 3858215 3858216 3858217 3858218 3858219 3858220 3858221 3858222 3858223
load: load1 9.04 is above 9.0 after 85s
  load1 9.04 is above 8: the flood benches skip themselves
step 0: the tree, the sha and the host — ok
step 1: check-docs.sh (ci job: docs) — ok (0s)
step 2: check-orchestration.sh (ci job: docs) — ok (26s)
step 3: nix flake check (ci job: check) — ok (3s)
step 4: cargo fmt (ci job: check) — ok (3s)
step 5: cargo clippy (ci job: check) — ok (11s)
step 6: test suite — ok (245s)
  observed skip lines: 14 (the count is
  not the signal; the list is)
  adding the load-conditional list: /mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/gate/wt/scripts/expected-skips/local-under-load.txt
  expected list: /mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/gate/wt/scripts/expected-skips/local.txt
  the skip list matches the expectation exactly.
step 7: the skips — ok
  load1 9.04 at the start, 11.70 at the end
gate: GREEN sha=ede24518457b68051b467bd9b362bc53c9a20208 steps=0-8:ok host=servernixos load1=9.04
load: the command exited 0; load1 now 11.70
load: killed spinners 3858215 3858216 3858217 3858218 3858219 3858220 3858221 3858222 3858223
load: load1 after cleanup 11.70
```

**Run E — plain, at rest, on this branch's tip.** It runs after this file
is committed, because a handoff is itself a docs file and a gate run
before it was written tested a different tree. Its receipt will be
`<worktree>/target/herd-gate/<tip sha>.receipt`, and **its verbatim
verdict is in the report to the orchestrator, not here** — `herd-merger`
step 6 settles this shape for the same reason. `target/` is gitignored
and this worktree is temporary, so that report is the durable copy.

`check-orchestration.sh`: **243 passed, 0 failed** at step 2 of every run
above. ORCH-111's `:464` case did not fire once in four runs.

### The skips, each one named (D27)

12 at rest, and all 12 are one capability: this host grants no cgroup v2
delegation, so a job may not create a cgroup under
`chatons.slice/chatons-herd-claude-831000.scope`.

```
1. `a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device`
2. `a_declared_ceiling_changes_what_the_inert_weight_line_says`
3. `a_job_cannot_fork_past_its_own_pid_ceiling`
4. `a_job_s_cgroup_carries_the_cpu_io_and_pid_bounds`
5. `a_job_s_cgroup_carries_the_hard_io_ceiling_it_was_given`
6. `a_job_s_record_carries_the_pressure_it_ran_under`
7. `an_allocation_past_the_ceiling_kills_the_job_and_only_the_job (the named memory outcome)`
8. `an_io_weight_that_cannot_bite_is_said_out_loud (the weighted-device branch)`
9. `a_tighter_enclosing_memory_max_lowers_the_room_that_is_announced`
10. `no_declared_ceiling_leaves_io_max_empty`
11. `no_declared_quota_leaves_cpu_max_at_the_kernel_s_max`
12. `the_runner_says_which_memory_ceiling_it_enforces (the cgroup branch of the report)`
```

14 above load1 8: those same 12, plus `flood-fifty` and
`flood-two-fifty`, which read the host's load and refuse to flood a
machine shared with a production system. Not one cgroup refusal moved
between the two measurements, which is what makes the pair readable
rather than merely green.

`scripts/report-skips.sh` prints every one of them as a skip and the gate
counts every one as a skip. None is reported as a pass.
## Risks and findings

### A `--bless` run leaves a GREEN receipt, and `pre-push` would take it

The sharpest thing I found, and it is not hypothetical — two such
receipts exist in my worktree's store right now.

`--bless` manufactures the expectation and then compares the run
against it, so step 7 always matches and the run ends GREEN and writes
a receipt like any other. Run A's receipt says `verdict: GREEN sha:
7eb9585…` for a tree that **does not contain `local.txt`** — a plain
gate on that sha is REFUSED, exit 3. The receipt records no trace of
`--bless`, and `--verify-receipt` reads three things only: `verdict:
GREEN`, the `sha:` line, and that the file exists. So
`pre-push`'s check 1b would accept a blessing run's receipt as the
gate for a landing's second parent.

The receipt does carry `expected-skips: <path>`, and in run B's case
that path is in my scratchpad, outside the repository altogether. A
reader can see it; nothing mechanical does.

Recommendation, smallest first: **a `--bless` run writes no receipt.**
Its job is to produce a list, not to certify a sha, and it cannot
honestly do both — the list it certifies against is not in the tree it
gated. Three lines in step 8. If a receipt is wanted, it needs a
`blessed: yes` field that `--verify-receipt` refuses.

### One load reading decides the list, and the flood benches take two

`gate.sh:138` reads `load1` once, at step 0, and that single figure
decides whether `local-under-load.txt` is consulted. The flood benches
read the load twice: at the start of each flood
(`fake_hub_flood.rs:364`) and again mid-flood (`:511`, the
`THE LOAD GUARD BIT` path), and either reading over 8 produces the
skip.

So a gate run that starts quiet and gets busy — another agent beginning
a compile, which is routine with 85 worktrees on this host — produces a
flood skip that is not on the allowed list, and the gate goes **RED on
a clean tree, for a reason that is not in the diff**. It costs a whole
suite run to find out, and the retry is honest only if the host happens
to be quiet again.

Measured, from my own four runs: `load1` rose 0.32 → 2.36 in run A and
9.56 → 14.91 in run B. The end-of-run figure is routinely several
points above the start, and the gate already prints it as
`load1-end`. It just does not use it to choose the list.

Recommendation: apply the load-conditional list when **either** reading
is above `HERD_GATE_LOAD_NOTE`, and say in the log which reading
decided. That is a one-line change to a comparison that already has
both numbers in hand.

### `--bless` cannot write the file the brief asked me to bless

`--bless` writes `$HERD_GATE_EXPECTED` and nothing else (`gate.sh:45`,
`:215-227`). There is no `--bless-under-load`. A second `--bless` run
under load, which is what the brief describes, therefore **overwrites
`local.txt`** with the 14-name list.

That mistake is silent-green, which is why it is worth a row: with all
14 in `local.txt`, an at-rest run finds the two flood names missing,
and a missing expected skip is a `note`, never a failure
(`gate.sh:259`). The gate stays GREEN while a load-gated bench is
skipped on a quiet host with nobody told. The split exists to stop
exactly that.

What I did instead: `HERD_GATE_EXPECTED=<scratch> ./scripts/gate.sh
--bless` under load, then committed the two lines whose names are not
in `local.txt`. Recommendation: either a `--bless-under-load` flag, or
two sentences in `expected-skips/README.md` naming the env-var
procedure, because the next agent will reach for `--bless` and get it
wrong the same way.

### `--check-install` still advises the value ORCH-131 removed

`scripts/githooks/pre-push:152`, on the path where the setting is
missing, prints `Fix: git config core.hooksPath scripts/githooks` — the
relative form. An agent who follows its own error message puts ORCH-131
straight back, repository-wide.

And `--check-install` cannot tell the two apart: it matches
`*scripts/githooks*` against `git rev-parse --git-path hooks`, which
both forms satisfy. `check-orchestration.sh:1171-1176` already asserts
this as a known limit ("answers about the config, not the directory"),
so the limit is documented — but it means **nothing mechanical catches a
regression to the relative value**, and before today it reported
`installed` in a worktree where the hook file did not exist (place 3
above, measured).

Recommendation: `--check-install` refuses a `core.hooksPath` that does
not start with `/`, and its fix line names the absolute path. One
`case` statement, and it would have caught ORCH-131 the day it was set.

### The receipt store is per-worktree, so my GREEN is not transferable

`receipts=${HERD_GATE_RECEIPTS:-$root/target/herd-gate}` (`gate.sh:44`)
and `target/` is gitignored, so the receipt run E writes lives in my
worktree and `herd-merger`'s worktree cannot see it. The merger must
run the gate on its own integration tip, which its step 7 already
says. Worth stating plainly because "a green gate for the second
parent" can read as something a branch can hand over, and it is not.

### The handoff template contradicts itself about the gate

`templates/handoff.md` says the gate "is the **last thing run** — after
this file is committed" and, in the same paragraph, that "the output
under *Gates* is that run's". Both cannot hold: a committed file cannot
contain the output of a run that has not happened. `herd-merger.md`
step 6 resolves it for the merger — the handoff names the receipt the
gate will write, the verbatim output goes in the report — and that is
what this handoff does. Recommendation: the template says so for every
agent.

### Where the brief was wrong, and where it was right

- **"three merges behind" understates it by an order of magnitude.**
  `860c4f5..7eb9585` is **28 merge commits on main's first-parent line**,
  265 commits, 67 merges in all. The last three were today's batches,
  which is probably what was meant; the checkout predated PR #76. It
  changes nothing about the act — `--ff-only` was always going to work —
  but it is why that checkout's `scripts/` was far too old for a
  relative `core.hooksPath` to resolve, which is item 2's whole premise.
- **The step timings are optimistic.** At rest I measured docs 1 s,
  orchestration 18 s, flake 1 s, fmt 2 s, clippy 18 s, suite 240 s;
  under load, orchestration 35 s and suite 257 s. Budget **~5 minutes
  per gate run**, not the ~3.5 the brief's figures add up to, and
  remember a bootstrap needs several.
- **ORCH-139 is right, and its diagnosis is exactly right.** I left
  `TMPDIR` alone (`/mnt/ssd_980/herd/claude/tmp`, 28 characters) and saw
  no `SUN_LEN` failure in four runs. My worktree path is 122 characters
  and that was harmless, which confirms the fault is `TMPDIR`
  specifically and not a long path anywhere.
- **"12 at rest, 14 above load1 8, the two extra being the flood
  benches and not cgroup refusals" is exactly right**, measured both
  ways, and the delta is `flood-fifty` and `flood-two-fifty` and
  nothing else.
- **ORCH-111's `:464` case did not fire** in any of the four runs:
  `check-orchestration: 243 passed, 0 failed` every time.
## Decisions needed

None that blocks this branch. Two want an owner, both above and both
about `gate.sh` rather than about this work:

1. **A `--bless` run writes a GREEN receipt that `pre-push` would
   accept.** Recommendation: a blessing run writes no receipt.
   Fallback: a `blessed: yes` field that `--verify-receipt` refuses.
2. **One `load1` reading at step 0 decides which list applies, while
   the flood benches read the load again mid-run.** Recommendation:
   apply the load-conditional list when either reading is over the
   threshold. Fallback: say in `workflow.md` that a landing waits for a
   quiet host, which is the weaker half of ORCH-135's own suggestion.

## Resume point

The gate goes green on this host now, so what follows is a landing and
not more bootstrapping. For `herd-merger`: integrate this branch, and
run the gate on your own integration tip — the receipts here sit in a
gitignored `target/` inside a temporary worktree and cannot travel.

Then the rows this leaves open, none of which this task may write:
ORCH-135's last step, **add the skip list to `workflow.md`'s transition
step 3 as a sixth precondition**, and the findings above as rows.

## Processes

All of them my own children, killed by PID and never by name:

- 11 `nice -n 19` CPU spinners for run B, PIDs 3794139-3794149, all
  reaped — verified with `kill -0`, nothing alive.
- 9 for run D, PIDs 3858215-3858223, all reaped, verified the same way.
- Four `gate.sh` runs, each the foreground of its own background shell,
  each exited on its own.

`nice -n 19` rather than a cargo build, deliberately: it raises `load1`,
which is what the flood benches read, while leaving the suite and this
host's production the CPU whenever they want it. Nothing else on the
host was signalled. No `pkill`, no kill by name.

`CARGO_TARGET_DIR` was `…/scratchpad/ct`, outside the worktree, and is
deleted once the branch is pushed. The worktree's own `target/` holds
only the gate's four logs and its receipts — no cargo output went there
— and stays until the orchestrator has read them.
