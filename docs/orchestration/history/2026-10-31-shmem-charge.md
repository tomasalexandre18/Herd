# shmem-charge — handoff 2026-10-31

- **Item:** ORCH-312 (new; drafted below)
- **Agent:** herd-step
- **Branch / commit:** `shmem-charge` / see the push, based on origin/main
  `d32c87a7876b91c55ca5caa8284ba5fa1357adc7`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

**The question is settled, by measurement, on this host: a tmpfs byte a
job writes IS charged to that job's own `memory.max`.** So `memory_mib`
(E-29) and `tmpfs_mib` (E-31) are **not** the independent quantities
Herd admits them as.

Three arms, one transient systemd scope each, `MemoryMax=64M` and
`MemorySwapMax=0` — the runner's own job-cgroup shape
(`herd-runner/src/cgroup.rs`: `memory.max`, then
`memory.swap.max = 0`). Same tool, same quantity, three kinds of memory.
2026-10-31, on `servernixos`:

| arm | 192 MiB of | cgroup `shmem` | `oom_kill` | `dd` | verdict |
|---|---|---|---|---|---|
| A, known-bad fixture | anonymous | 0 | **1** | 137 | killed |
| B, **subject** | tmpfs, `/dev/shm` | **62988288** | **1** | 137 | **killed** |
| C, negative control | ext4 page cache | 0 | 0 | 0 | survived, 192 MiB written |

**The line that decides it** is arm B's `shmem 62988288` read out of the
scope's own `memory.stat`: 60.07 MiB of tmpfs pages accounted *inside
the cgroup that wrote them*, with `memory.peak` at exactly 67108864 —
the ceiling — and the file stopped at 62988288 bytes of the 201326592
asked for. Arm C is what makes this a result rather than a coincidence:
it hit the same ceiling **170 times** (`max 170`) and was never killed,
because page cache is reclaimable. Arm A is the detector proved on a
known-bad fixture before arm B's silence could have been trusted. The
difference between B and C is reclaimability, not size.

**Written into the tree as a test**
(`herd-runner/tests/confinement.rs`):
`a_tmpfs_byte_a_job_writes_is_charged_to_its_own_memory_ceiling`, which
runs the same experiment through the real path — hub, runner, job — and
gives a **three-way** verdict: *charged* (`stopped_by_cap{memory}`,
asserted), *not charged* (a named `panic!`, not a bare assertion), or
*the premise did not hold* (a `HERD-SKIPPED` line with its reason).

**`docs/runner-confinement.md`** now says **thirteen** where it said
twelve, and the in-scope table measured on 2026-10-30 keeps its own
denominator of twelve and now says so in words.

**`scripts/expected-skips/local.txt`** gains the thirteenth name.

## Not done, and why

- **No admission arithmetic, no cap and no default was changed**, and
  `herd-proto` was not touched: the brief measures, it does not redesign.
  The per-job ceiling itself is ORCH-312, drafted below.
- **`docs/status.md` and `docs/coverage.md` still say twelve**, and both
  are fenced by the brief (`status.md` beyond a dead pointer, 90 lines,
  D276; `coverage.md` is held by another branch). Each is now false by
  one number. That is **ORCH-313**, drafted below, not an edit made here.
- **`scripts/expected-skips/ci.txt` was not touched.** It records what
  `ubuntu-latest` skipped on one green run; adding a line nobody observed
  there would be writing a false observation into an evidence file. CI
  does not compare against it (`.github/workflows/ci.yml` runs
  `report-skips.sh`, not `gate.sh`), so nothing goes red; the name
  arrives at the next CI bless.
- **Which cgroup is charged when two cgroups share one tmpfs file was
  not measured.** The fixture shows the *writer's* cgroup is charged. See
  *Risks and findings*.

## Gates

`./scripts/check-docs.sh`, run on the tree before this file was
committed:

```
docs ok
```

**The first gate run was RED, on `cargo fmt` alone, and is recorded
here rather than discarded.** Herd job **`j0000000175`** on sha
`2d45b6b52d1f30f93fa4280e69ac54dd52d4edc2`:

```
gate: RED sha=2d45b6b52d1f30f93fa4280e69ac54dd52d4edc2 failed=4:cargo-fmt skips=under-load(flood-fifty=8.15>8,flood-two-fifty=8.15>8)
gate: NOT GREEN — this sha must not be pushed to main.
```

Two diffs, both in the new block, both layout and neither logic:
rustfmt wanted the third tuple of the exit-code array exploded, and
wanted `assert_eq!`'s `*limit, SHMEM_CEILING,` packed onto one line
(the opposite of what was guessed — rustfmt packs an `assert_eq!`'s
non-string arguments). Applied exactly as rustfmt printed them. **No
test, expectation or assertion was changed to make a gate green.**

What that red run already proves, because the gate runs every step
whatever fails: **steps 5, 6 and 7 passed on this code.** Clippy was
clean under `-D warnings`, the suite was green, and step 7 said *"the
skip list matches the expectation exactly"* — so the new test ran,
skipped on this host, and its thirteenth name matched
`scripts/expected-skips/local.txt` as added.

The **last run**, on the tree as finally committed, is
`./scripts/gate.sh` bare and whole, submitted through Herd the same way
(`--label build`, `--memory-mib 4700`, `ws://127.0.0.1:8791`, an
absolute `cd` into this worktree and `CARGO_TARGET_DIR=/mnt/ssd_980/
herd/build/shmem-charge`). Its verbatim verdict, its job id and its
receipt path are in the agent's report, not in this file: a second
commit to carry them would need a third gate run.

**The amendment after that green run is prose only, and the Rust gates
were NOT re-run.** It corrects finding 10, which was false, and extends
one board row; `git diff --name-only` against the gated tree lists
`docs/` and nothing else — no `.rs`, no script, no manifest. So the four
Rust gates cannot be affected by it: the gated sha `eb09e0a` already
carries the identical `herd-runner/tests/confinement.rs` and
`scripts/expected-skips/local.txt`, green at steps 0-8. What was re-run
is what the change can break:

```
docs ok
check-orchestration: 324 passed, 0 failed
```

Worth recording, because it nearly cost a wrong conclusion: the first
attempt at that board row put two literal `|` characters inside a
markdown cell, which silently turned a 10-field row into 12, and the
over-long line it introduced made **14** of `check-orchestration`'s own
fixture assertions fail at once — every one of them downstream of a
single 81-column line, because that script runs `check-docs.sh` against
a copy of this tree. A count of failures pointed nowhere; the one line
did.

**Skips:** this host has no delegated cgroup v2, so the new assertion
**joins the standing family and is printed as skipped, never as passed**
(D27, invariant 5). The list goes from twelve names to **thirteen**:

```
HERD-SKIPPED: a_tmpfs_byte_a_job_writes_is_charged_to_its_own_memory_ceiling
```

Its reason is premise 2 — the `RLIMIT_AS` fallback — and the skip says
what would settle it: a delegated scope, by the recipe in
`docs/runner-confinement.md`.

The at-rest list is thirteen; the run itself observed **fifteen**, the
two extra being `flood-fifty` and `flood-two-fifty`, each excused by
name out of `scripts/expected-skips/local-under-load.txt` because the
bench's own line read `8.15>8`. Neither is this task's.

## Risks and findings

1. **The brief's claim that `dev/shm` appears in no runner test needed a
   qualifier.** It is true of `herd-runner/tests/` — `shm` and `tmpfs`
   matched **no** file there — but false of the crate: `herd-runner/src/
   metrics.rs:519` `the_tmpfs_room_is_measured_or_named_but_never_zero`
   already reads `/dev/shm` through `DEFAULT_TMPFS_PATH`. It measures
   *room*, never *charging*, so the brief's conclusion stands and only
   its scope was wider than its evidence.
2. **Herd names a job no tmpfs, and the test does not pretend otherwise.**
   `job_environment` (`herd-runner/src/confine.rs`) points `TMPDIR`,
   `TMP`, `TEMP` and `XDG_RUNTIME_DIR` at the workspace on the data root,
   which is ordinary disk — so a test writing to `$TMPDIR` would measure
   page cache and nothing else, which is exactly arm C and exactly what
   survives. What a job *can* reach is `/dev/shm` by absolute path,
   because M4 gives a job no mount namespace and `herd-runner/src/root.rs`
   says so in those words. That mount is also `DEFAULT_TMPFS_PATH`, the
   one E-31 measures, so the fixture is on the quantity the admission
   arithmetic actually uses — not a hypothetical one.
3. **The workspace walk cannot perturb this fixture, by construction.**
   `restore_tree` starts at the workspace and `restore_ancestors` returns
   early unless the path is under the root it was given
   (`herd-runner/src/root.rs`). The fixture is on `/dev/shm`, outside the
   data root entirely. Nothing was proved about the walk; it was placed
   out of its reach.
4. **The fixture removes its own bytes, and must.** A charged verdict
   means the job is killed **mid-write**, so the job runs no cleanup:
   `OnTheHostsTmpfs`'s `Drop` is what keeps tens of mebibytes off the
   host's shared tmpfs after each run. The probe's own files were removed
   and `/dev/shm` is back to `0` used.
5. **One false-skip path remains, named in the test.** If
   `memory.oom.group` ever fails to take (it is written with `let _ =`),
   `dd` would die while the shell outlived it and the job would exit 91 —
   read as *the premise did not hold*, not as *not charged*. Conservative,
   and the skip's own text says so.
6. **What the fixture does not measure: a shared tmpfs file.** Shmem is
   charged to the cgroup that **first faults a page in**. The three arms
   show the writer's own cgroup is charged; no arm has two cgroups
   touching one file. So if Herd ever puts anything shared on a tmpfs —
   a content store, a pack — job A could be charged for pages job B then
   reads, and a per-job ceiling would be unpredictable for a reason this
   measurement does not cover. Worth its own measurement before any
   shared tmpfs ships.
7. **The consumer's report and this measurement are two different
   proofs, and both were needed.** Theirs is on `/mnt/ssd_980/chatons/
   ram`, a mount **outside** Herd's `--host-tmpfs` path, so it proves the
   kernel's charging rule and not Herd's bookkeeping — they said so
   themselves. This one is on the mount E-31 measures. Nothing of theirs
   was logged into; their figures are their report (invariant 9, D161).
8. **`systemd-run --user` works on this host once `XDG_RUNTIME_DIR`
   points at the real bus.** `docs/runner-confinement.md` says a plain
   login session has no delegation; here the user bus is at
   `/run/user/1002/bus` and the session's `XDG_RUNTIME_DIR` is
   overridden to `/mnt/ssd_980/herd/claude/run`, which hides it. With
   `XDG_RUNTIME_DIR=/run/user/1002` a delegated scope is available, which
   is how the three arms were run and how the thirteen in-scope
   assertions could be made. **Nothing was written into `/sys/fs/cgroup`
   by hand**: systemd created each scope and removed it, and the only
   access taken there was reading.
9. **A gate run in such a scope needs `-p MemoryMax=`**, as
   `docs/orchestration/gotchas/a-delegated-scope-blinds-the-preflight.md`
   requires; both probe and recipe carry it.

10. **`./scripts/bench-preflight.sh` is a working gate: it exits `1` on
    REFUSED. The danger is in the caller, not the script.** This entry
    said the opposite until it was corrected on 2026-10-31, and the
    correction is kept here with its cause because the true version is
    worth more than silence.
    - **The script.** `scripts/bench-preflight.sh:395`-`:396` is
      `printf 'bench-preflight: REFUSED, do not start the heavy
      phase.\n' >&2` then `exit 1`. Measured directly, output to files
      and the status read on its own: `DIRECT EXIT=1` at load 13.40,
      with the refusal on stderr.
    - **The trap.** `$?` after a pipeline is the **last** command's
      status, so a caller that pipes the preflight reads `tail`'s `0`
      and sails straight through the refusal. Measured at the same
      instant as the line above: `PIPE EXIT=0`. Proved on a known-bad
      fixture too — `sh -c 'exit 1' | tail -1` gives `0`, and
      `set -o pipefail` gives `1`.
    - **How the false version was reached**, since the method is the
      lesson: the three refusals read at loads 14.27, 10.22 and 11.97
      were all taken through `| tail` or `| grep`, so the script's own
      status was never read; and the one *direct* reading that returned
      `0` was taken at an instant when the load had already fallen to
      about 5 and the script was **clearing**, not refusing. A true
      observation — exit `0` — was carried under a false qualifier,
      *"even when it prints REFUSED"*. The verdict and the status have
      to be read from **one** invocation, which is what the two
      measurements above do.
    - **The script must not be changed.** It is correct, and rewriting
      a working guard on a bad measurement is how a safety mechanism
      gets broken. What is owed is a line for callers, and the row
      for it already exists: **ORCH-229** — this is its fourth
      instance, and the first by a pipeline.

    This task never set or exported
    `HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB`, and waited for
    `ok, safe to start.` before each heavy phase.

## Drafted for `herd-merger`

### The `docs/decisions.md` entry, in full

```markdown
## 2026-10-31 — D-NEXT A job's tmpfs bytes are charged to its own
memory ceiling, so E-29 and E-31 are not independent

**Context.** Herd admits `memory_mib` (E-29) and `tmpfs_mib` (E-31)
separately, as two independent quantities. Alexa decided per-job
ceilings would follow **measurement**; this is one of the two
measurements. Nothing in the tree covered it: `shm` and `tmpfs` matched
no file in `herd-runner/tests/`.

**Measured on 2026-10-31, on `servernixos`, three arms under one 64 MiB
ceiling with `memory.swap.max = 0` — the runner's own job-cgroup shape
(`herd-runner/src/cgroup.rs`). Same tool, same quantity, three kinds of
memory:**

| arm | 192 MiB of | cgroup `shmem` | `oom_kill` | `dd` | verdict |
|---|---|---|---|---|---|
| anonymous (known-bad fixture) | anon | 0 | 1 | 137 | killed |
| **tmpfs, `/dev/shm`** | shmem | **62988288** | **1** | 137 | **killed** |
| ext4 (negative control) | page cache | 0 | 0 | 0 | survived, all 192 MiB |

**Decision: a tmpfs byte a job writes is charged to that job's own
`memory.max`, and the two admitted quantities are not independent.** The
deciding line is `shmem 62988288` read out of the writing cgroup's own
`memory.stat`, with `memory.peak` at exactly the 67108864 ceiling and the
file stopped at 62988288 of the 201326592 bytes asked for. The negative
control is what makes it a result: it hit the same ceiling 170 times
(`max 170`) and was never killed, because page cache is reclaimable
while shmem under `swap.max = 0` is not. The difference is
reclaimability, not size.

**What follows for a per-job ceiling.** It must be a function of
`memory_mib` **+ `tmpfs_mib`** + margin. A ceiling of `memory_mib +
margin` would OOM-kill, on its first day, every job that reserves tmpfs
and fills it.

**Necessary and not sufficient.** The consumer's `leash-stick-update`
declared 1792 MiB and reserved 2.5 GiB of tmpfs, and used 4.031 GiB anon
and 3.930 GiB shmem — **2.30×** and **1.57×**. A ceiling correctly
computed as `memory_mib + tmpfs_mib + margin` would still have killed
it. Counting tmpfs is the formula's half; the declarations are the
consumer's, and they know it.

**Two proofs, not one twice.** The consumer's memcg OOM report for
j0000000069 (`usage 8388608kB, limit 8388608kB … shmem 4219834368`,
with the killed `qemu`'s own `shmem-rss` at **0** — the charge is at the
cgroup, not the process) is on `/mnt/ssd_980/chatons/ram`, a mount
**outside** Herd's `--host-tmpfs` path, so it proves the kernel's rule
and not Herd's bookkeeping; they said so themselves. The arms above are
on `DEFAULT_TMPFS_PATH`, the mount E-31 measures. Their figures are
their report about their own machine (invariant 9, D161).

**Written into the tree as a test, with a three-way verdict**:
`a_tmpfs_byte_a_job_writes_is_charged_to_its_own_memory_ceiling`
(`herd-runner/tests/confinement.rs`) — *charged*, *not charged* (a named
`panic!`), or *the premise did not hold*. Four ways the premise can fail
each get a skip of their own rather than a pass: no tmpfs to write in; no
delegated cgroup v2, where `RLIMIT_AS` bounds one process and a file
write never trips it, so the job would survive for the wrong reason; a
ceiling that does not kill the same quantity of anonymous memory, checked
on every run as a known-bad fixture so that a surviving subject is only
read once the detector has fired; and a job that never wrote a byte. This
host has no delegation, so the assertion skips here and the expectation
list goes from twelve names to **thirteen**.

**Not decided here.** No admission arithmetic, cap or default moved, and
which cgroup is charged when two cgroups share one tmpfs file was not
measured — the arms show the *writer's* cgroup is charged, and nothing
more (ORCH-312).
```

### The `board.md` rows, in full

Two rows were **added** to `docs/orchestration/board.md` on this branch,
under *Owed*, after `ORCH-311`, and one existing row was **extended**.
All three are in the file verbatim, which is what the merger reads.

- `ORCH-312` — the per-job ceiling design, with this measurement as its
  input.
- `ORCH-313` — the two counted sentences in `docs/status.md` and
  `docs/coverage.md` that this brief fenced and that are each now false
  by one number.
- `ORCH-229`, **extended, not duplicated** — the existing row about
  callers that do not act on the preflight's exit status. This task is
  its fourth instance and the first by a pipeline, and the first in
  which the misreading was reported as a defect of the script. No
  `ORCH-314` was opened: the board is one list, and a second row on the
  same class would fragment it.

## Decisions needed

None for Alexa. The measurement settles a *how*, and per-job ceilings
were already his decided direction. **One thing to report to him, with
no question attached** (CLAUDE.md, outcome 3): a per-job ceiling that
counts only `memory_mib` would kill every tmpfs-using job on day one,
and counting tmpfs is necessary but not sufficient while the consumer's
declarations run 2.3× and 1.57× over.

## Resume point

`herd-planner` sizes ORCH-312: the per-job ceiling as a function of
`memory_mib + tmpfs_mib + margin`, with the second measurement Alexa
asked for. To re-make the thirteen in-scope assertions here, including
this one against a real ceiling:

```sh
export XDG_RUNTIME_DIR=/run/user/1002   # the real user bus; the session
                                        # default hides it
systemd-run --user --scope -p Delegate=yes -p MemoryMax=12G \
  <CARGO_TARGET_DIR>/debug/deps/confinement-<hash> \
  --test-threads 1 --nocapture
```

## Processes

PIDs started: three transient `systemd-run --user --scope` probes, each
exited on its own; no PID was signalled, and nothing was killed. 3006543,
3021002, 565152 and 662711 were not touched. No `pkill` in any form.

Own build tree: `CARGO_TARGET_DIR` under `/mnt/ssd_980/herd/build/`,
removed once the branch was pushed; its size is in the agent's report.
