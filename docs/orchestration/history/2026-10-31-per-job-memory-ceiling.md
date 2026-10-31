# per-job-memory-ceiling — handoff 2026-10-31

- **Item:** ORCH-312 (drafted below; the task itself was briefed directly by
  Alexa on 2026-10-31 as the project's first priority, and closes the half
  D180 left open)
- **Agent:** herd-step
- **Branch / commit:** `per-job-memory-ceiling` / this commit, based on
  `origin/main` `d32c87a7876b91c55ca5caa8284ba5fa1357adc7`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger` names
  the landing here, and fills the record of `templates/landing.md`

## Done

**The defect, as briefed, is real. Every link was verified before anything
was changed; all eleven citations hold, with the two line numbers noted
below.**

| Claim | Verified at |
|---|---|
| `FromHub::AssignJob` carried no requirements field | `herd-proto/src/lib.rs:2375`, seven fields: `job_id`, `attempt`, `command`, `declaration`, `timeout_s`, `packs_in`, `packs_out` |
| `Declaration` is command + commit + args + executor | `herd-proto/src/declaration.rs:235` |
| `Requirements` holds `memory_mib` and `tmpfs_mib` | `herd-proto/src/declaration.rs:516` |
| and is parsed in the **hub** | `herd-hub/src/server.rs:4448`, `herd_proto::Requirements::read(&log)` |
| the runner knew nothing of it | `grep -rn requirements herd-runner/src/` → no match, exit 1 |
| the hub's own comment | `herd-hub/src/server.rs:1642`, verbatim: *"the runner's arithmetic also uses the cap it puts on every job, which no frame carries"* |
| the only `memory.max` write in the product | `herd-runner/src/cgroup.rs:660` |
| inside `JobCgroup::create(base, tag, limits)` | `herd-runner/src/cgroup.rs:644` |
| `memory.swap.max` at 0 | `herd-runner/src/cgroup.rs:663` |
| one `limits`, built once at startup | `herd-runner/src/main.rs:525`-`:530` → `confine.rs:180` `job_limits()` → `confine.rs:430` `Ceiling::detect(...)` → `cgroup.rs:603` `open_for_job` |
| one caller | `herd-runner/src/job.rs:359`, and `grep -rn open_for_job` finds that one plus a single unit test |
| `memory_need_mib` answers `Ok(None)` for a real class | `herd-hub/src/job.rs:1690`, the doc comment names `herdctl submit -- true` |
| `jobs_at_once` | `herd-runner/src/confine.rs:284` |
| the hub must count the same set as the runner | `herd-hub/src/matcher.rs:684`, in the comment above the `jobs_at_once` arm |
| D180 rejected mirroring the hub's arithmetic and called carrying the figure *"a protocol surface change and therefore Alexa's call"* | `docs/decisions.md:8921`-`:9020`, read in full |

Two line numbers in the brief are one or two lines off and name the right
construct: the `memory.max` write is at `cgroup.rs:660` as briefed but
`JobCgroup::create` begins at `:644`, and `fn open_for_job` is at `:603`
with its body's `let Ceiling::Cgroups` at `:604`. Nothing turned on either.

**What now exists.**

1. **`herd_proto::JobNeed`** (`herd-proto/src/declaration.rs`): `memory_mib`
   and `tmpfs_mib`, with `ceiling_mib`, `ceiling_bytes` and `line`. Named
   `JobNeed` and not `Granted` because `herd_runner::endpoint::Granted`
   already exists and means a capability the hub pushed (D209) — two
   `Granted`s in one tree would be a reviewer's trap.
2. **`need: Option<JobNeed>` on `FromHub::AssignJob`**, `#[serde(default,
   skip_serializing_if = "Option::is_none")]`. **On the frame, not inside
   `Declaration`** — the justification is in the field's own doc comment and
   in the drafted entry below: `Declaration` is `None` for every undeclared
   job, and `herdctl submit --memory-mib 4700 -- cargo build`, this project's
   own gate line, is a job with a memory figure and no declaration.
3. **`herd_proto::CEILING_MARGIN_PERCENT = 10`**, one named constant on the
   contract so the hub's packing (ORCH-312) charges the same ceiling without
   a second copy of the number. Its doc comment carries the whole reason.
4. **The hub fills it** (`herd-hub/src/server.rs`, `need_for` beside
   `dispatch`): off the record, with the `-j` the attempt was given, so a
   `labels_from` reading's answer wins over the submission's (D74). Three
   answers collapse to two — a declared figure travels, `Ok(None)` travels as
   nothing, and `Err` (a per-thread figure with no `-j`, D82) travels as
   nothing too, because a ceiling invented out of half a figure would be
   worse than the one the machine's owner lent.
5. **The runner holds the job to it** (`herd_runner::job_ceiling`,
   `herd-runner/src/confine.rs`): `JobCeiling { memory_max, need, floored }`.
   `job::execute` clones the `Confinement` and replaces its one memory figure,
   so `memory.max`, the fallback's `RLIMIT_AS` and the figure reported with
   `stopped_by_cap { memory }` are **one number by construction** — which is
   also why nothing in `job.rs`'s OOM step attribution was touched: those two
   sites (`job.rs:1093`, `job.rs:1612`) read `caps.memory_max` and became
   correct without an edit.
6. **`Ceiling::open_for_job(&tag, memory_max)`** takes the job's figure and
   overrides that one field of the startup `JobLimits`. The other five bounds
   are the machine's and are untouched.
7. **A fallback host says so, per job.** A job the hub costed at nothing gets
   no line (its ceiling is the one the startup report named). A job that
   declares a figure on a host with no delegated subtree gets a line carrying
   `NOT ENFORCED`, the host's own reason and what is in force instead —
   `RLIMIT_AS` on the first process, which bounds neither the tree nor
   resident memory.

**`f(X)`, with its margin and the written reason.**

```
ceiling_bytes = max( ((memory_mib + tmpfs_mib) * 110 / 100) * 2^20 ,
                     MINIMUM_MEMORY_MAX )        # 16 MiB
```

- **Both figures, because they are not independent.** Tmpfs pages are charged
  to the cgroup that writes them and `memory.swap.max` is 0, so they are
  unreclaimable and sit **inside** the ceiling. `j0000000069`: `limit
  8388608kB` with `shmem 4219834368` of it, the killed process's own
  `shmem-rss` at 0 — 3.93 GiB of tmpfs inside an 8.00 GiB ceiling. A job
  declaring `memory_mib=4096 tmpfs_mib=4096` declares exactly that 8.00 GiB,
  so a memory-only ceiling of 4505 MiB would have killed it sooner.
- **The margin is 10 %, one constant, with one reason.** A ceiling of the
  declared sum *exactly* is the figure that killed that job: the last page of
  a reservation the job is entitled to use is the page that trips the limit.
  So nought is not available. **Why a proportion and why one tenth:** the
  report's three figures do not close on its limit — 8.00 GiB less 4.03 GiB
  of anon and 3.93 GiB of shmem leaves about 40 MiB, a residue of that order
  at that rounding, and the only measurement of it there is. That residue is
  what the kernel charges the cgroup on the job's behalf and no declaration
  can see: page tables, slab, kernel stacks, socket buffers. It scales with
  the job's footprint and its process count, not with the host. One tenth is
  an order of magnitude above it, which is the side to be wrong on for a cap
  meant to kill a runaway job and not a working one, and small enough that
  ORCH-312's packing pays a tenth of a job for it rather than a whole one.
  **Provenance:** the three GiB figures and the two kernel strings are the
  brief's quotation of the consumer's own report; the ~40 MiB residue is
  arithmetic on those rounded figures and is marked as such in the code.
- **The floor is `MINIMUM_MEMORY_MAX` (16 MiB)**, which is a measurement —
  `/bin/sh -c 'echo ok'` needs that much address space — and
  `Confined::prepare` already refuses a *global* figure under it because
  invariant 2 asks for a cap and not for a wall. A per-job figure under it is
  the same wall arriving from the hub. The job is **not** refused for it; the
  figure is raised and the line says so.

**The protocol version: no bump, and the reasoning is the constant's own, not
the brief's.** `herd-proto/src/lib.rs`, in `PROTOCOL_VERSION`'s doc comment
(the paragraph headed *"What is NOT foreclosed, which is worth saying because
it was once believed to be"*), lists three additive shapes and the first is
verbatim:

> * a new `#[serde(default)]` field on an existing variant, when its absence
>   means exactly what its default says (the precedent is `JobMetrics`'
>   `pressure`, D73);

and the three that still move it are *a new `ToHub` variant*, *a new
`FromHub` variant a **requester** can see* and *removing a field, or
re-meaning one, anywhere* (D124, D128). A second statement of the same rule
is at `herd-proto/src/lib.rs:559`: *"A defaulted field does not move
`PROTOCOL_VERSION` (D124, D128): an older peer's frame still decodes, and
this one's reading of it is the behaviour that peer already had."* This field
is exactly that shape and absence means what a pre-field runner already does,
so **`PROTOCOL_VERSION` stays 3** and nothing was stopped and reported.

**Prose corrected in the same commit, because the change falsifies it (D279's
discipline).** `docs/runner-confinement.md`: the `--job-memory-max` table row,
a new *Whose figure the ceiling is* subsection, and the *"Invariant 2 still
holds either way"* paragraph, which was true of the cap and false of the
job's own ceiling on a fallback host. `docs/architecture.md:1006`, the
`memory.max` node of the confinement diagram. `docs/joining.md:591`, the row
that called `--job-memory-max` "the kernel ceiling on a single job".

## Not done, and why

1. **The slot arithmetic.** `jobs_at_once` (`herd-runner/src/confine.rs:284`)
   is byte-for-byte unchanged, on the brief's instruction, so that one gate
   run can tell which change broke what. The consequence is real and is
   **ORCH-312**, drafted below: a runner now permits the sum of its jobs' own
   ceilings while still counting seats by `--job-memory-max`.
2. **`docs/coverage.md`** is untouched (forbidden by the brief). E-29, E-30
   and E-31 each now have more behind them than their cells say; recounting a
   must between columns is a judgement about the consumer's acceptance and not
   an execution agent's call (the shape of ORCH-288).
3. **`docs/decisions.md`** is untouched. The entry is drafted below under
   `D-NEXT`; the merger allocates the number.
4. **`docs/status.md`** is untouched — not even a pointer: it is at its exact
   90 lines (D276) and nothing in it was falsified by this change.
5. **`herdctl/src` and `herd-runner/src/job.rs`'s OOM step attribution** were
   not entered; two other agents hold them. Nothing was needed in either: the
   `Confinement` clone makes `job.rs:1093` and `job.rs:1612` report the
   per-job figure without an edit, and `herdctl` needs no change because
   `Declared::memory_mib` already exists and already reaches the record.
6. **A tmpfs reservation stated without a memory figure still travels as
   nothing.** `ORCH-314`, drafted below. It is today's behaviour unchanged and
   unreachable from a `labels_from` reading (which demands all four keys and
   refuses `memory_mib=0`), so it is a row and not a defect in this landing.
7. **No end-to-end assertion that the **kernel** enforces the per-job figure.**
   This host has no delegated cgroup v2 subtree — it is the family the twelve
   standing skips of `scripts/expected-skips/local.txt` belong to — so what is
   asserted here is *which number Herd writes into `memory.max`*, read back
   out of the file, plus the whole chain end to end under the `RLIMIT_AS`
   fallback. The claim that the kernel then kills on it is where it already
   was: `an_allocation_past_the_ceiling_kills_the_job_and_only_the_job`, whose
   named-outcome branch is one of the twelve.

## Gates

**Run before this commit, verbatim.**

`nix develop --command cargo fmt --all --check`:

```
(no output; exit 0)
```

`nix develop --command cargo clippy --workspace --all-targets -- -D warnings`
(the worktree prefix, `/mnt/ssd_980/herd/claude/tmp/…/scratchpad/wt`, is
elided from each path and nothing else is):

```
    Checking herd-proto v1.0.0 (.../per-job-memory-ceiling/herd-proto)
    Checking herd-runner v1.0.0 (.../per-job-memory-ceiling/herd-runner)
    Checking herdctl v1.0.0 (.../per-job-memory-ceiling/herdctl)
    Checking herd-hub v1.0.0 (.../per-job-memory-ceiling/herd-hub)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 20.38s
```

`./scripts/check-docs.sh`:

```
docs ok
```

The eleven new tests. Seven were run alone by name; the other four — the
remaining `declaration::tests` — ran inside `cargo test -p herd-proto --lib`,
which is the line quoted under them. Their known-bad fixtures are in the
controls table below, not here:

```
test declaration::tests::a_ceiling_clears_the_sum_that_killed_j0000000069 ... ok
test declaration::tests::the_tmpfs_reservation_is_inside_the_ceiling ... ok
test declaration::tests::an_impossible_figure_saturates_rather_than_wrapping ... ok
test declaration::tests::the_line_names_both_figures_and_the_ceiling ... ok
test what_a_job_was_costed_at_rides_the_assignment_and_an_older_runner_ignores_it ... ok
test confine::tests::a_costed_job_gets_its_own_ceiling_and_an_uncosted_one_gets_the_machine_s ... ok
test confine::tests::a_declaration_under_the_floor_is_floored_and_says_so ... ok
test confine::tests::a_host_that_cannot_enforce_a_per_job_ceiling_says_so_per_job ... ok
test cgroup::tests::each_job_s_cgroup_carries_the_ceiling_that_job_was_costed_at ... ok
test server::tests::an_assignment_carries_a_declared_figure_and_never_a_guessed_one ... ok
test a_job_runs_under_the_ceiling_it_declared_and_not_its_runner_s ... ok
```

`cargo test -p herd-proto --lib` in full: `93 passed; 0 failed; 0 ignored`.


**Controls — every detector proven on a known-bad fixture before its silence
was trusted.** Each fixture was applied, the named test run alone, the test
seen RED, and the fixture reverted. A test that passes with the change removed
has proved nothing.

| Known-bad fixture | Test | What it printed |
|---|---|---|
| `CEILING_MARGIN_PERCENT = 0` | `a_ceiling_clears_the_sum_that_killed_j0000000069` | `FAILED. 0 passed; 1 failed` at `declaration.rs:1106` — the assertion that a ceiling of the declared sum is the figure that killed the job |
| the figure removed from the frame under test (`need: None`) | `what_a_job_was_costed_at_rides_the_assignment_and_an_older_runner_ignores_it` | `panicked … both figures have to be on the wire … {"type":"assign_job","job_id":"j1","attempt":1,"command":["true"]}` |
| `open_for_job` ignores the figure it is given (`let _ = memory_max;`, the pre-change behaviour) | `each_job_s_cgroup_carries_the_ceiling_that_job_was_costed_at` | `panicked … the ceiling in the cgroup must be this job's own, not the machine's 2 GiB` |
| `need_for` returns `None` for every job | `an_assignment_carries_a_declared_figure_and_never_a_guessed_one` | `panicked … assertion left == right failed` |
| `job_ceiling` ignores the job's figure (`let asked = global;`) | `a_costed_job_gets_its_own_ceiling_and_an_uncosted_one_gets_the_machine_s` | `panicked … 8192 MiB declared plus the margin, and not the machine's 2 GiB` |
| the same fixture | `a_job_runs_under_the_ceiling_it_declared_and_not_its_runner_s` (end to end) | `panicked … a job that declared 512 MiB was stopped by its runner's own 32 MiB: Exited { code: 2 } / /bin/sh: xrealloc: cannot allocate 8388736 bytes` |

The last row is the one that matters: with the figure not reaching the runner,
the shell dies inside the 32 MiB the machine lent, naming the allocation. With
it reaching the runner the same script holds its whole 32 MiB string. The
fallback host's own sentence, printed by the green run of that test and quoted
verbatim, is the evidence for point 4:

```
herd-runner: job j0000000001 declares 512 MiB of memory and 0 MiB of tmpfs,
which is 563 MiB of ceiling once the 10% margin is on, and this host CANNOT
enforce a per-job ceiling: cgroup v2 delegation is unavailable: this runner
cannot create a cgroup under /sys/fs/cgroup/chatons.slice/chatons-herd-claude-256608.scope
(Permission denied (os error 13)). Run it as a systemd unit with Delegate=yes,
or inside a delegated scope, and point --cgroup-root inside that subtree. What
it gets instead is an address-space limit of 563 MiB on its first process,
which bounds neither the job's process tree nor its resident memory — so the
ceiling this job was admitted against is NOT ENFORCED here
```

and the hub's own line from the same run, which is the other half of the
chain:

```
herd-hub: j0000000001 is attempt 1 on runner m4-declared-ceiling: … admitted
512 MiB of memory, 6.5 GiB free of 7 GiB after it (8 GiB lent, 1 GiB kept as
its own floor); it reserves no tmpfs …
```

**The last run, named and not quoted**, on this committed tree:
`./scripts/gate.sh`, bare and whole, submitted through Herd to
`ws://127.0.0.1:8791` with `--label build --memory-mib 4700` and this
worktree's own `CARGO_TARGET_DIR`. Its receipt goes to
`target/herd-gate/` inside this worktree, which is `gate.sh`'s own
`HERD_GATE_RECEIPTS` default; its verdict and job id are in this agent's
report, and the output is **not** in this file: a handoff is a
docs file, and a gate run made before it was written tested a different tree.

**Skips.** **This branch introduces none, and changes none.** The eleven new
tests were sited so that this host can make every one of their assertions,
and the three `scripts/expected-skips/` lists are untouched: the twelve
standing `HERD-SKIPPED` lines of `local.txt`, the two load-conditional names
of `local-under-load.txt` and the thirty-eight `#[ignore]` names of
`ignored.txt` are all exactly as they were on `d32c87a`.

**What any one run of the gate joins is a property of the host at that
minute, not of this branch, so it is in the agent's report and not here.**
The two names in `local-under-load.txt` — `flood-fifty` and
`flood-two-fifty` — are load-gated benches that decline when they read the
host's own one-minute load above their own ceiling of 8, and on a host shared
with the consumer's production they often do. A run that joins them has
joined two skips: printed as skipped, named with the load they read, and
never counted as a pass (D27, invariant 5).

## Risks and findings

1. **A declaration is now binding downwards.** `ORCH-313`, drafted below.
   A job that declares **less** than its runner's `--job-memory-max` is now
   held to the smaller figure. On the consumer's server, where
   `--job-memory-max` is `8GiB` (`docs/joining.md:527`), a job whose reading
   answers `memory_mib=2048 tmpfs_mib=0` gets a 2252 MiB ceiling where it used
   to get 8 GiB. **This is the intent of the change and the precondition of
   ORCH-312** — a runner cannot pack by a figure it does not enforce — but it
   is the one way this landing can make a job fail that passed the day before.
   It is reported, not asked (CLAUDE.md, outcome 3), and the reversal is one
   line: `max(f(declared), --job-memory-max)` in `job_ceiling`.
   **Not measured against their declarations:** Herd is shipped, not operated
   (D161), so nobody here knows what their reading answers. The one declared
   sum we do know, `j0000000069`'s, is **helped** (9011 MiB against the 8192
   that killed it).
2. **The two halves of invariant 2 now count different memory.** `ORCH-312`.
   D180's arithmetic was *"the cap every job is actually permitted is what is
   counted"*, and after this branch that is no longer one number. On a runner
   lending 25 GiB at `--job-memory-max 2GiB`, twelve seats can hold twelve
   jobs declaring 4 GiB and permit 52 GiB. **The brief ordered this split**
   and the next task closes it; until it lands, a fleet whose jobs declare
   more than their runner's figure is less protected by the seat count than
   it was, and more protected by the ceiling.
3. **`RLIMIT_AS` is now set from the per-job figure on a fallback host, which
   can be tighter than before.** `RLIMIT_AS` bounds virtual address space, not
   resident memory, and a runtime that reserves address space generously (a
   JVM, a Go binary, `qemu`) can map far more than it touches. A job declaring
   a small figure on a fallback host may therefore fail an allocation it would
   have made under the old global cap. One figure was preferred to two
   deliberately — two would mean the runner reporting a ceiling other than the
   one it writes — and the job's own line says the figure is `NOT ENFORCED` as
   a tree ceiling. The floor at 16 MiB is what keeps the degenerate case out.
4. **Field name.** `JobNeed`, not `Granted`: `herd_runner::endpoint::Granted`
   is the capability the hub pushed (D209), and the brief's word "granted"
   would have put two unrelated `Granted`s in one tree. The frame's field is
   `need`, which does not collide with M11's pack `needs` on `Asked` —
   `AssignJob` has no `needs` field, it has `packs_in` and `packs_out`.
5. **One premise of the brief was narrowed, not refuted.** The brief says
   `Requirements` "is parsed in the **hub**"; it is, at `server.rs:4448`, and
   the type itself lives in `herd-proto`, which is why `JobNeed` could be put
   beside it on the contract rather than invented in the hub.

6. **Found while building the end-to-end test, and it is a trap, not a
   defect: a runner started through the library announces no memory floor, so
   the hub admits no costed job to it at all.** `ORCH-315`, drafted below.
   `herd_runner::metrics::host_size()` answers a total and a thread count and
   no floor; `HostSize::admissible_memory_mib()`
   (`herd-proto/src/placement.rs:130`) needs both; the matcher's memory arm
   then answers `Admission::NotHere("… does not say how much memory it keeps
   back for itself …")` and the job waits for a runner that will never exist.
   `herd-runner/src/main.rs` fills the default in before it dials — D180's own
   sentence — so **production is unaffected**. Measured: the first version of
   the end-to-end test waited **eight minutes with zero CPU time and no child
   process** before it was killed by PID. `Harness::lending` now exists in
   `herd-runner/tests/confinement.rs` and says why in its doc comment.

## Drafted for `herd-merger`

### The `docs/decisions.md` entry, in full, headed `D-NEXT`

Paste verbatim, with the number the merger allocates in place of `D-NEXT`:

```markdown
## 2026-10-31 — D-NEXT A job's declared memory need reaches the runner, and
the kernel ceiling is per job

**Alexa's decision, of tonight.** D180 (2026-10-25) closed its own entry by
naming this and declining to take it: *"the runner cannot: `AssignJob` carries
no declared memory need, only a command … That is a protocol surface change
and therefore Alexa's call, and it is deliberately not in this entry."* The
call has been made, and it is the project's first priority.

**Context, measured.** The hub reads a job's memory figure off the record and
admits the job against it (`JobRecord::memory_need_mib`, E-29); the runner is
the only thing in the product that writes a kernel memory ceiling
(`herd-runner/src/cgroup.rs`, one `memory.max` write, one caller); and the
frame between them carried the job id, the attempt, the command, the
declaration, the time cap and the packs, and **no requirements field at all**.
`grep -rn requirements herd-runner/src/` answered nothing. So every job on a
machine ran under one figure — `--job-memory-max`, resolved once at startup
and handed to every job — whatever it had declared and whatever it had been
admitted against. The hub's own comment said so: *"the runner's arithmetic
also uses the cap it puts on every job, which no frame carries."*

The consumer's kernel memcg report for their job `j0000000069`, on a Herd job
cgroup, is what this costs: `memory: usage 8388608kB, limit 8388608kB … shmem
4219834368`, with the killed `qemu`'s own `shmem-rss` at **0** — 4.03 GiB of
anon plus 3.93 GiB of tmpfs against an 8.00 GiB ceiling that was the runner's
global figure and not the job's.

**Choice.** Four things.

1. **One optional field on `AssignJob`**, not inside `Declaration`:
   `need: Option<JobNeed>`, `#[serde(default, skip_serializing_if = …)]`,
   carrying `memory_mib` and `tmpfs_mib`. On the frame for the reason
   `timeout_s` is on the frame: a `Declaration` is what the requester's file
   said and is `None` for every undeclared job, while these figures come off
   the **record**, where a `labels_from` reading replaces what the submission
   carried (D74) — and `herdctl submit --memory-mib 4700 -- cargo build`,
   which is this project's own gate, is a job with a memory figure and no
   declaration at all. Inside `Declaration` the figure would be lost for
   exactly the jobs that state it without declaring a command.
2. **The runner writes `memory.max` from the job's figures when it has them
   and from `--job-memory-max` when it does not.** The global flag is not a
   leftover: `memory_need_mib` answers `Ok(None)` for a real class —
   `herdctl submit -- true` — admitted on its capabilities alone, so the
   machine's figure is the only one anybody stated about it. One figure for
   the uncosted, each job's own for the costed.
3. **The ceiling is a function of both figures: `(memory_mib + tmpfs_mib)`
   plus one margin of 10 %**, floored at `MINIMUM_MEMORY_MAX`. The tmpfs is
   inside it because tmpfs pages are charged to the cgroup that writes them
   and `memory.swap.max` is 0, so they are unreclaimable: Herd admits E-29 and
   E-31 as independent quantities and they are not independent. A ceiling of
   the declared sum **exactly** is the 8.00 GiB that killed `j0000000069`, so
   nought is not an available margin. The margin is one named constant,
   `herd_proto::CEILING_MARGIN_PERCENT`, so that the hub's packing can charge
   the same ceiling without a second copy of the number; it is a proportion
   because what it covers scales with the job — the report's own figures leave
   about 40 MiB of 8 GiB unaccounted, which is page tables, slab, kernel
   stacks and socket buffers, charged to the cgroup and visible to no
   declaration.
4. **A host with no delegated cgroup v2 says the ceiling is NOT ENFORCED, per
   job.** The fallback is `RLIMIT_AS` on the job's first process: one
   process's address space, not a process tree's resident memory. A runner
   that printed the figure and left it there would be reporting a ceiling it
   does not hold, so the job's own line carries the words, the host's reason
   and what is in force instead.

**No `PROTOCOL_VERSION` bump, and the rule is the constant's own.**
`herd-proto/src/lib.rs`'s doc comment on `PROTOCOL_VERSION` lists three shapes
that are additive, and the first is *"a new `#[serde(default)]` field on an
existing variant, when its absence means exactly what its default says (the
precedent is `JobMetrics`' `pressure`, D73)"*; the three that still move it
are a new `ToHub` variant, a new `FromHub` variant a requester can see, and
removing or re-meaning a field (D124, D128). Absence here means *this job was
costed at nothing*, which is what a runner that has never heard of the field
already does. The number stays 3, `RECORD_VERSION` stays where it is, and a
runner built before this field decodes the assignment and ignores the key —
asserted in `herd-proto/tests/round_trip.rs` against a struct shaped like the
older runner's, the way `Hello`'s `jobs_at_once` is asserted a few tests
above. That compatibility is not decoration: the first consumer migrated to
v1.0.0 and must be able to move a hub without moving every runner in the same
hour.

**Alternative rejected — put the figures on `Declaration`.** It is where
`Requirements` already lives, and it is the wrong place for the reason in
item 1: `Declaration` is `None` for an undeclared job, and an undeclared job
with a memory figure is not a corner case but this project's own gate.

**Alternative rejected — `memory_mib` alone.** A ceiling of the memory figure
plus a margin kills every job that uses the tmpfs it reserved, on the first
day it is enforced: `j0000000069` reached 3.93 GiB of shmem, charged to its own
cgroup, and the brief's own arithmetic shows a memory-only ceiling of 4505 MiB
against 8030 MiB of real usage.

**Alternative rejected — `max(f(declared), --job-memory-max)`**, so that the
per-job ceiling only ever rises. Safer for the consumer by one failure mode —
a job that under-declares would go on running — and it forecloses packing by
the granted sum, which is the next task: a runner cannot pack by a figure it
does not enforce, and D180's own arithmetic is that what is counted must be
what is permitted. The consequence is recorded as a board row rather than
hidden: a job declaring less than its runner's figure is now held to the
smaller one.

**What this costs, and it is not nothing.** `jobs_at_once` is unchanged, so a
runner now permits the sum of its jobs' own ceilings while still counting
seats by `--job-memory-max`: twelve seats of 2 GiB can hold twelve jobs
declaring 4 GiB. That is the next task and it needs the hub counting the same
set (`herd-hub/src/matcher.rs` is explicit), and it was left out of this
branch so that one gate run can tell which change broke what. No exit code
moved — `rc 124` remains one code for all four caps — no admission arithmetic
moved, and no default figure moved.

**Reversibility.** High. Nothing was written to disk, no record shape moved,
no frame is new and no version number moved. Removing the per-job ceiling is
removing one argument from `Ceiling::open_for_job` and one call to
`job_ceiling`; a fleet with one runner reverted and one not differs only in
which ceiling each writes, and the hub is indifferent to both.

```

### The `board.md` rows, in full

All three are **already written into `docs/orchestration/board.md` on this
branch** — ORCH-312 in *Owed*, ORCH-313 and ORCH-314 in *Open* — and are
repeated here verbatim so the merger can act on them without reading the
diff.

**ORCH-312** (section *Owed — agreed, not started*):

| ID | Item | Origin | Owner | State | Next action | Depends on | Evidence |
|---|---|---|---|---|---|---|---|
| ORCH-312 | **A runner now permits the sum of its jobs' own ceilings while `jobs_at_once` still divides what it lends by one global figure, so the two halves of invariant 2 no longer count the same memory.** As of `per-job-memory-ceiling`, `memory.max` is each job's own — `(memory_mib + tmpfs_mib) + 10%`, `herd_proto::CEILING_MARGIN_PERCENT` — while `herd_runner::jobs_at_once` (`herd-runner/src/confine.rs`) is unchanged and still answers *(lent − floor) ÷ `--job-memory-max`*. D180's own arithmetic was "the cap every job is actually permitted is what is counted", and that is no longer one number: on a runner lending 25 GiB at `--job-memory-max 2GiB` the count is twelve, and twelve jobs each declaring 4 GiB are now permitted 52 GiB. **It was left out of that branch deliberately, on the brief's instruction, so that one gate can tell which change broke what** — and the branch changed nothing in the slot arithmetic, in admission or in any default figure | `herd-step` on `per-job-memory-ceiling`, 2026-10-31, from the brief's own point 5 | herd-step | owed | Pack by the **granted sum**: hold at most the jobs whose ceilings fit in what this machine lends, less its floor, and have the hub charge the same figure — `herd-hub/src/matcher.rs` is explicit that a hub counting a different set from the runner is the defect, so the two sides move together. The margin is already one named constant in `herd-proto` for exactly this, so neither side needs a second copy. The figure to charge is `JobNeed::ceiling_mib`, not `memory_mib`, or the host is over-committed by the margin times the job count | `per-job-memory-ceiling` landing | `herd-runner/src/confine.rs` `jobs_at_once` and `job_ceiling`; `herd-proto/src/declaration.rs` `JobNeed`, `CEILING_MARGIN_PERCENT`; `herd-hub/src/matcher.rs` the `jobs_at_once` arm; **D180**, **D181**, **D182**; `docs/orchestration/history/2026-10-31-per-job-memory-ceiling.md` |

**ORCH-313 and ORCH-314** (section *Open — seen, not agreed or sized*):

| ID | Item | Origin | Owner | State | Next action | Depends on | Evidence |
|---|---|---|---|---|---|---|---|
| ORCH-313 | **A declaration is now binding downwards as well as upwards, and that is a change the first consumer can see.** `per-job-memory-ceiling` writes `memory.max` from the job's own figures whenever the hub costed it, so a job that declares **less** than its runner's `--job-memory-max` is now held to the smaller figure: on their server, where `--job-memory-max` is `8GiB` (`docs/joining.md:527`), a job whose reading answers `memory_mib=2048 tmpfs_mib=0` gets a 2252 MiB ceiling where it used to get 8 GiB. That is the point of the change — the figure admission weighs a job against is the figure the kernel holds it to, and it is what ORCH-312 needs in order to pack honestly — but it is also the one way this landing can make a job fail that passed the day before, and the failure mode is `rc 124` with `stopped_by_cap { memory }`, not a mystery. **Not measured against their declarations:** Herd is shipped and not operated (D161), so nobody here knows what their `labels_from` reading answers; what is known is the one figure in their own report, `j0000000069`, whose declared sum came to exactly the 8 GiB that killed it, and that job is **helped** by the change (9011 MiB) rather than hurt | `herd-step` on `per-job-memory-ceiling`, 2026-10-31, derived from the brief's own point 2 and the figures in `docs/joining.md` | orchestrator (to report), then Alexa if he wants it reversed | open | **Report it, with no question attached** (CLAUDE.md, outcome 3): the direction is deliberate and reversible in one line. The cheap reversal, if a job of theirs starts failing, is `max(f(declared), --job-memory-max)` in `herd_runner::job_ceiling` — the ceiling then only ever rises — at the cost of ORCH-312, which cannot pack by a figure the runner does not enforce. Do not reverse it pre-emptively: a declaration nothing enforces is a declaration nobody will keep true | nothing | `herd-runner/src/confine.rs` `job_ceiling`; `herd-proto/src/declaration.rs` `JobNeed::ceiling_mib`; `docs/joining.md:527` and `:624`; **D161**; `docs/orchestration/history/2026-10-31-per-job-memory-ceiling.md` |
| ORCH-314 | **A tmpfs reservation declared with no memory figure reaches the runner as nothing, so it is charged to a cgroup whose ceiling never counted it.** `herd_proto::JobNeed` carries both figures or neither, and `need_for` (`herd-hub/src/server.rs`) sends it only when `memory_need_mib` answers a figure — so `herdctl submit --tmpfs-mib 4096 -- <command>` with no `--memory-mib` travels with no `JobNeed` at all and the job runs under `--job-memory-max`, with 4 GiB of unreclaimable shmem charged inside it. **Unreachable from a reading**, which is why it is a row and not a defect in the landing: `herd_proto::Requirements::read` demands all four keys and refuses `memory_mib=0`, so every job whose requirements came from a `labels_from` reading has both figures. It is reachable only from a bare submission that states one and not the other, and this is today's behaviour unchanged — the landing made it visible, not worse | `herd-step` on `per-job-memory-ceiling`, 2026-10-31, read off the two paths | herd-step | open | Decide which of the two it is. Either the hub **refuses** a submission that reserves tmpfs without stating a memory need, by name, the way it refuses `memory_mib` and `build_thread_mib` given together — the honest reading, since a reservation nobody costed is one admission cannot weigh — or `JobNeed` carries `memory_mib: Option<u64>` and the runner adds the reservation to the global figure, which changes a default figure for a class of job and is therefore not an execution agent's call. **The first is cheaper and is a refusal, not a silent ceiling** | ORCH-312 | `herd-hub/src/server.rs` `need_for`; `herd-hub/src/job.rs` `memory_need_mib`, `tmpfs_need_mib`; `herd-proto/src/declaration.rs` `Requirements::read`, `JobNeed`; **E-29**, **E-31**; `docs/orchestration/history/2026-10-31-per-job-memory-ceiling.md` |


**ORCH-315** (section *Open — seen, not agreed or sized*, beneath ORCH-314):

| ID | Item | Origin | Owner | State | Next action | Depends on | Evidence |
|---|---|---|---|---|---|---|---|
| ORCH-315 | **A runner started through the library announces no memory floor, so the hub admits no job that declares a memory need to it — and the failure is a silent wait, not a refusal.** `herd_runner::metrics::host_size()` answers `HostSize::new(total_memory_mib(), threads)` and **no floor**; `HostSize::admissible_memory_mib()` is `Some(lent − floor)` only when **both** are there, so it is `None`, and `herd-hub/src/matcher.rs`'s memory arm then answers `Admission::NotHere("… does not say how much memory it keeps back for itself, so a <n> MiB job cannot be admitted there")`. `herd-runner/src/main.rs` fills `herd_proto::default_memory_floor_mib` in before it dials, which is D180's own sentence — *"the same default the binary fills in before it dials, so the figure a runner enforces is the figure it announced"* — so **production is unaffected**; what is affected is every test that starts a runner with `herd_runner::Config::default()`, which is all of them. Measured 2026-10-31 on `per-job-memory-ceiling`: a job declaring `memory_mib = 512` against such a runner waited **eight minutes with zero CPU** and no child process, until the test was killed; `Harness::lending` in `herd-runner/tests/confinement.rs` now exists so a test can say the figure, and names the reason | `herd-step` on `per-job-memory-ceiling`, 2026-10-31, measured by the hang and then read off `metrics.rs`, `placement.rs` and `matcher.rs` | herd-step | open | Decide where the default belongs. **Recommended: move it into `Config`'s own default or into `herd_runner::run`**, so the library and the binary announce the same machine — the asymmetry is the kind D23 forbids in its own domain, a figure that is read one way by one caller and another way by another. The cheaper half, which costs nothing and should happen either way: have `metrics::host_size()` say in its doc comment that it names no floor and that the caller must. Not urgent for the product, and a trap for the next agent writing a test about a declared figure | nothing | `herd-runner/src/metrics.rs` `host_size`; `herd-proto/src/placement.rs:130` `admissible_memory_mib`; `herd-hub/src/matcher.rs` the memory arm's `None` branch; `herd-runner/src/main.rs`; `herd-runner/tests/confinement.rs` `Harness::lending`; **D180**, **D23**; `docs/orchestration/history/2026-10-31-per-job-memory-ceiling.md` |

## Decisions needed

**None blocking.** The one protocol decision this task needed was made before
it started, and the version question it was told to settle settled itself
against a bump, with the citation above — so nothing was stopped and nothing
is waiting.

One thing is **reported with no question attached**, which is ORCH-313: a
declaration is now binding downwards, so a job that under-declares is held to
its own figure. The recommendation is to leave it as it is and watch, because
a declaration nothing enforces is a declaration nobody will keep true; the
reversal is one line if a job of the consumer's starts failing.

## Resume point

**ORCH-312, and it is the next task by the brief's own words:** pack by the
granted sum. Both sides at once — `herd_runner::jobs_at_once` holding at most
the jobs whose `JobNeed::ceiling_mib` fits in what the machine lends less its
floor, and `herd-hub/src/matcher.rs` charging the same figure over the same
set (D159, D169), because that file's own comment says a hub counting a
different set from the runner is the defect. The margin is already one
constant on the contract, so neither side invents a number. The exact first
command:

```sh
git worktree add -b pack-by-the-granted-sum <path> origin/main
```

after this branch has landed, and the first thing to read is
`herd-runner/src/confine.rs` `jobs_at_once` beside `job_ceiling` — they are
twenty lines apart and answer the same question with two different figures.

## Processes

PIDs started: one `nix develop` shell per phase (`cargo check`, `cargo fmt`,
`cargo clippy`, the named `cargo test` runs and their controls), each ended
by its own exit, and three backgrounded waiters on
`./scripts/bench-preflight.sh` and on log files.

**Two PIDs were signalled, both my own, both by PID**: `950099` (a
`timeout nix develop` wrapper) and `977266` (the `confinement` test binary
under it), when the first version of the end-to-end test hung on admission —
see ORCH-315. `SIGTERM` first, and the process was gone before the `SIGKILL`
branch was reached. No `pkill`, no `killall`, nothing by name, and no PID of
another agent or of the consumer's production: `3006543`, `3021002`, `565152`
and `662711` were never addressed.

`CARGO_TARGET_DIR` was `/mnt/ssd_980/herd/build/per-job-memory-ceiling`,
never the root filesystem and never the shared `target/`. Its size at the end
and its removal are in this agent's report: the tree is deleted once the
branch is pushed, and the gate that reads it is submitted through Herd before
that (gotcha `build-weight-off-root`).
