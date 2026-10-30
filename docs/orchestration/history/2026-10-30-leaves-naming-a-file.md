# leaves-naming-a-file — handoff 2026-10-30

- **Item:** drafted, unnumbered (highest on `main`: ORCH-282)
- **Agent:** herd-step
- **Branch / commit:** `leaves-naming-a-file` / this commit, based on
  origin/main `df47266`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## The answer, first

**The file still comes back. Declaring a plain file in `leaves` costs the
collection nothing.** The first consumer's seven declarations are
harmless and **this is not a release blocker.** Measured on
`m11-pack-layout` (`ae1188f`), against the real hub, runner and `herdctl`
as processes, with a control.

`results.json` and `status.json` were written into the directory
`herdctl wait --outputs-to` was handed in **all three** cases, byte for
byte identical, and both files are named in each job's record
`outputs.files`. The only difference a `leaves` naming a file makes is
**one extra sentence on `outputs.skipped`** and the same sentence on
`herdctl`'s stderr. It does **not** move the exit code: all three
invocations exited 0.

## Done

### What was measured, and against which tree

One `tokio` test, `a_file_named_in_leaves_still_comes_back_as_an_output`,
run inside a Herd job (`j0000000119`, `queue-local`, build slot 1 of 2,
`--label build --memory-mib 4700`, 49 s wall) against a detached
worktree at `ae1188f`. Three declared commands, one hub and one runner,
all real processes; the fixture's step writes `results.json` and
`status.json` under `{out}` and nothing else:

```toml
[commands.verdict]
line = "sh tools/herd/step.sh verdict --both"
timeout_s = 600
leaves = { "results" = "results.json" }

[commands.control]
line = "sh tools/herd/step.sh control --both"
timeout_s = 600

[commands.nightly]
line = "sh tools/herd/step.sh nightly --both"
timeout_s = 600
leaves = { "status" = "status.json", "results" = "results.json" }
```

`verdict` reproduces the consumer's six commands, `nightly` their
seventh, `control` is the same command with **no `leaves` at all**.

**The branch was the right one to measure, proven and not assumed.** On
`origin/main`, `herdctl/src/declaration.rs` carries no
`leaves: self.leaves.clone()` at all — `grep -c` answers **0** — so a
declared `leaves` never reaches a runner and the whole path is inert.
On `ae1188f` it answers **1**. The measurement is therefore only
meaningful on `ae1188f`, which is where it was taken.

### The result, verbatim

The three cases side by side, as the test printed them:

```text
================ THE ANSWER ================
control (no leaves)      job j0000000002 exit 0
  --outputs-to: [
    "results.json (88 byte(s))",
    "status.json (36 byte(s))",
]
  record files: [
    "results.json (88 byte(s))",
    "status.json (36 byte(s))",
]
  record skipped: []
verdict (leaves=results.json) job j0000000003 exit 0
  --outputs-to: [
    "results.json (88 byte(s))",
    "status.json (36 byte(s))",
]
  record files: [
    "results.json (88 byte(s))",
    "status.json (36 byte(s))",
]
  record skipped: [
    "this job declared that it leaves \"results\" at \"results.json\", and that is not a directory; a pack is a directory holding manifest.tsv, pack.meta and objects/",
]
nightly (two file keys)  job j0000000004 exit 0
  --outputs-to: [
    "results.json (88 byte(s))",
    "status.json (36 byte(s))",
]
  record files: [
    "results.json (88 byte(s))",
    "status.json (36 byte(s))",
]
  record skipped: [
    "this job declared that it leaves \"results\" at \"results.json\", and that is not a directory; a pack is a directory holding manifest.tsv, pack.meta and objects/",
    "this job declared that it leaves \"status\" at \"status.json\", and that is not a directory; a pack is a directory holding manifest.tsv, pack.meta and objects/",
]
```

And the consumer's own call, verbatim, for the declared case:

```text
===== verdict: herdctl wait j0000000003 --outputs-to =====
status: Some(0)
--- stderr ---
herdctl: job-id: j0000000003
herdctl: this job's output: this job declared that it leaves "results" at "results.json", and that is not a directory; a pack is a directory holding manifest.tsv, pack.meta and objects/
herdctl: wrote /mnt/ssd_980/herd/queue/runner-data/jobs/j0000000119/tmp/nix-shell.8nGE7v/.tmpHm16gY/verdict/outputs/results.json (88 bytes)
herdctl: wrote /mnt/ssd_980/herd/queue/runner-data/jobs/j0000000119/tmp/nix-shell.8nGE7v/.tmpHm16gY/verdict/outputs/status.json (36 bytes)
```

### The detector was proven before any negative was trusted

ORCH-267 is a row about `--outputs-to` returning nothing in silence, and
an empty directory and a directory nobody passed are the same picture.
Two proofs, both positive:

1. **The control ran first** and its assertion is written as the
   detector's own: a command with no `leaves` must have `results.json`
   in `--outputs-to`, and nothing below means anything until it passes.
   It passed — `herdctl: wrote <job-tmp>/control/outputs/results.json
   (88 bytes)`, the same line the declared case prints.
2. **The `leaves` path is proven to have fired**, not merely to have
   been declared: the "is not a directory" sentence is on the declared
   cases' records and **absent** from the control's. So `leaves` was
   read, acted on, and the file still came back.

### Why, in the source

`herd-runner/src/packstore.rs`, `leave_packs`: the declared path is
`symlink_metadata`'d, and a regular file falls into the `Ok(_)` arm,
which pushes the sentence and **`continue`s**. Nothing is renamed,
nothing is removed, and the walk in `herd-runner/src/outputs.rs`
`collect_record` then reads `{out}` with the file still in it. The
`skipped` sentences ride the record beside the walk's own lines and
`herdctl/src/main.rs` states in as many words that they "do **not** move
the exit code — D131's sentences are facts about one file and never a
verdict about the job."

## Not done, and why

- **No code change**, as the brief required: the answer is a
  refutation, so there is nothing to fix.
- **The measurement test is not landed.** It lives at
  `<scratchpad>/m11-measure/herd-hub/tests/leaves_a_file.rs` in a
  detached worktree at `ae1188f` and that path is session-scoped. The
  brief allowed no code change, and a test file is a code change; the
  declaration shape above is the load-bearing part and is quoted in
  full so it can be rebuilt. Landing it is the second drafted row's
  suggestion, not this task's act.
- **Nothing of the consumer's was read or touched** (invariant 9,
  D161). Their configuration reached this task as the orchestrator's
  report of their report, and the fixture's names are invented (E-11).

## Gates

Prose only — nothing outside `docs/` changed on this branch — so
`./scripts/check-docs.sh` alone, and **the four Rust gates were NOT
RUN** locally. What covers them is the batch's own `./scripts/gate.sh`
run at integration, which is `herd-merger`'s and not this agent's.

The last run is `./scripts/check-docs.sh` on the committed tree, after
this file was committed and before the push; its verbatim output goes
to this agent's report, not into the tree it gates.

The measurement job's own verdict, which is evidence and not a gate:
`j0000000119`, `cause: exited`, `passed: true`, `exit_code: 0`,
`exit_code_from: job`, `wall_time_ms: 49174`, on `queue-local`. The test
binary printed `test result: ok. 1 passed; 0 failed; 0 ignored`.

**Skips:** no skip. The test's own `HERD-SKIPPED` path — `git` missing
from the host — did not fire; the fixture repository was built and the
three jobs ran.

`./scripts/bench-preflight.sh` **refused the heavy phase at the first
ask** — *"load 9.09 is at or above 9.00 on 12 cores"*, with 14961 MiB of
slice room — and was obeyed as a gate: nothing was compiled or submitted
until it answered *"ok, safe to start"* at load 7.22 with 15131 MiB of
room. Its own note that all four figures are of that instant only
stands here too.

## Risks and findings

1. **A `leaves` naming a plain file is harmless; a `leaves` naming a
   DIRECTORY that is not a pack is not.** Read in
   `herd-runner/src/packstore.rs` and not measured here: a declared
   directory that `PackStore::ingest` refuses is handed to `set_aside`,
   which **renames it out of `{out}`** into the runner's refused
   directory and says so. It never reaches the record and never reaches
   `--outputs-to`, and "nothing in this runner reads it, offers it or
   gives it to a job again". So the consumer is safe because their seven
   declarations name files; anyone who declares a directory of ordinary
   result files would lose them. This is the substance behind the second
   drafted row and it is **source-read, not measured** — it deserves its
   own measurement before being quoted as a figure.
2. **The consumer will see new noise on every job.** Six commands print
   one `herdctl: this job's output: …` line each and `nightly` prints
   two, on every run, for declarations that are harmless. The verdict
   stands and the exit code is unchanged, but a wrapper that greps
   stderr for anything unusual will start matching.
3. **Two repositories out of two wrote a file where a pack directory
   was expected, and nothing said so until the field became
   behaviour.** Herd's own fixture, `herdctl/tests/fixture/mod.rs`,
   declared `leaves = { "grove-tree" = "tree.txt", "grove-log" =
   "logs/assemble.log" }` — two keys, both plain files — from commit
   `38a844f`, dated **2026-10-15**, the commit that built the fixture
   consumer repository M10 is judged against. It sat there for fifteen
   days and was discovered only when `m11-pack-layout`'s own gate went
   red on
   `herdctl::outputs::a_job_that_leaves_nothing_says_nothing_about_outputs`.
   The first consumer has the same mistake in **seven** places, with a
   comment saying why: the field was written as an output label before
   `leaves` meant a pack. Nothing in `herdctl` refuses or warns when the
   declaration is read: `Commands::read_in` copies `leaves` through
   unchecked, and the only sentence anyone gets is on a runner's stderr
   at execution time, after the job has run.
4. **The two-out-of-two figure is a count of repositories, not a rate.**
   Two is the whole population of repositories that declare `leaves`
   today, which is why it is stated as two out of two and not as a
   percentage.

## Drafted for `herd-merger`

No `docs/decisions.md` entry is drafted: the brief asked for none, the
measurement changes nothing and the question the finding raises is
Alexa's, not a decision this task may record.

**Two `board.md` rows.** Both are headed `ORCH-NEXT`; the merger
allocates the numbers and gaps stay (D148, invariant 8). Highest on
`main` at the time of writing: **ORCH-282**.

Row 1, for **`## Owed — agreed, not started`**, state `done` — the
answer in the tree, so it is not only a chat message:

| ORCH-NEXT | **Measured and refuted: a `leaves` naming a plain file does not stop that file coming back as an ordinary `{out}` output.** The first consumer's `tools/herd/commands.toml` has seven `leaves` declarations and every one names a FILE — `results = "results.json"` on `build`, `test-pack`, `test`, `test-affected`, `test-gpu` and `test-host`, and `{ status = "status.json", results = "results.json" }` on `nightly` — written as output labels before `leaves` meant a pack. Measured on `m11-pack-layout` (`ae1188f`), the branch that changed this, against the real hub, runner and `herdctl` as processes: `results.json` (88 bytes) and `status.json` (36 bytes) were written into the directory `herdctl wait --outputs-to` was handed in all three cases and are named in each record's `outputs.files`. **The seven declarations are harmless and this is not a release blocker.** The one and only difference is a sentence per key on `outputs.skipped` and the same sentence on `herdctl`'s stderr — `"… and that is not a directory; a pack is a directory holding manifest.tsv, pack.meta and objects/"` — which `herdctl/src/main.rs` states does not move the exit code; all three invocations exited 0. **The detector was proven before the negative was trusted** (ORCH-267): a control command with no `leaves` at all ran first and its file arrived, and the "is not a directory" sentence is on the declared cases and absent from the control, so `leaves` was read and acted on and the file still came back. Measured only on `ae1188f` because `origin/main`'s `herdctl` does not carry `leaves` onto `Asked` at all and the path is inert there. **The hazard that is real, source-read and not measured:** a declared DIRECTORY that is not a pack is renamed out of `{out}` by `set_aside` and reaches neither the record nor `--outputs-to`. | the first consumer's report of their own configuration, relayed by the orchestrator 2026-10-30; measured by `herd-step` the same day | herd-step | done | nothing — land the handoff with the batch; the instrument is not landed and landing it is the next row's suggestion | nothing | `docs/orchestration/history/2026-10-30-leaves-naming-a-file.md`; job `j0000000119` |

Row 2, for **`## Waiting — decisions for Alexa, one at a time`**, state
`waiting` — it changes what the product refuses:

| ORCH-NEXT | **Should a `leaves` that names a plain file be refused when the declaration is read, instead of earning a sentence on a runner's stderr after the job has run?** Two repositories out of two wrote a file where a pack directory was expected and nothing said so. Herd's own fixture declared `leaves = { "grove-tree" = "tree.txt", "grove-log" = "logs/assemble.log" }` from `38a844f`, dated 2026-10-15, and it was found fifteen days later only because `m11-pack-layout`'s gate went red on `a_job_that_leaves_nothing_says_nothing_about_outputs`. The first consumer has it in seven places, with a comment saying why: the field was written as an output label before `leaves` meant a pack. Two is the whole population of repositories that declare `leaves` today, so it is a count and not a rate. Today `herdctl` copies `leaves` through unchecked and the only sentence is the runner's, at execution time; the file itself is unharmed (measured, the row above), but a declared DIRECTORY that is not a pack is renamed out of `{out}` and lost to the requester, so the same mistake one shape over destroys results. This changes what the product refuses, which is why it is here and not decided. | the first consumer's argument, relayed by the orchestrator 2026-10-30; the two-out-of-two evidence measured and written by `herd-step` the same day | Alexa | waiting | **Recommended: refuse at declaration-read time, in `herdctl`, before a job is submitted.** `Commands::read_in` already refuses a declaration for other reasons and it is the one place that sees the file the requester wrote; a refusal there names the key, the path and the line, costs no job, and is the only moment at which the person who can fix it is still looking at it. A warning where the declaration lives is the weaker half of the same fix: it is read once and then never again. | If a refusal is too strong for a field this young, warn in `herdctl` at read time and keep the runner's sentence as it is — the behaviour then stays exactly as measured, and nothing in the consumer's seven declarations breaks either way | `docs/orchestration/history/2026-10-30-leaves-naming-a-file.md` *Risks and findings* 3; `herdctl/tests/fixture/mod.rs` at `38a844f`; `herd-runner/src/packstore.rs` `leave_packs` and `set_aside` |

## Decisions needed

**One, and it is Alexa's, not the orchestrator's.** It is drafted as
row 2 above and needs nothing else written: should a `leaves` naming a
plain file be refused when the declaration is read? Recommendation and
fallback are in the row. **Nothing is blocked on the answer** — the
measured behaviour is safe and the batch can land as it stands.

## Resume point

Nothing is owed on the measurement. For whoever continues: the two rows
above go on the board at integration, and the open follow-up is the
unmeasured half of finding 1 — submit a declared command whose `leaves`
names a **directory** of ordinary files and read whether
`--outputs-to` loses them, which is the case `set_aside` says it moves
aside. The instrument to copy is the declaration in *Done* above.

## Processes

Two background shells of this agent's own, both exited on their own:
the `bench-preflight.sh` poll and the `herdctl run` that followed it.
No process was killed, nothing was signalled, and the queue's runner
`662711`, our hub `565152` and the consumer's `3006543` and `3021002`
were not touched.

The build tree was `/mnt/ssd_980/herd/build/orch-283-leaves-outputs`,
**1.8G in the debug profile with tests** — one test target of one crate
and its 114 compiled dependencies, not the whole suite's 12–14 GB. It
is deleted; the deletion is reported with the push in this agent's
report. The two worktrees under this session's scratchpad are
`m11-measure` (detached at `ae1188f`, the instrument) and the branch's
own.
