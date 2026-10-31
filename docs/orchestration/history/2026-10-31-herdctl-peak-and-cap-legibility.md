# herdctl-peak-and-cap-legibility — handoff 2026-10-31

- **Item:** none assigned in the brief. This advances the **display half**
  of **ORCH-268** and files two rows of its own, drafted below as
  `ORCH-NEXT-A` and `ORCH-NEXT-B`.
- **Agent:** herd-step
- **Branch / commit:** `herdctl-peak-and-cap-legibility` /
  `bb242f7a26a9990d6592d96260d7ae2d10d2faa3`, based on origin/main
  `d32c87a7876b91c55ca5caa8284ba5fa1357adc7`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

Two figures Herd already recorded now reach the requester that submitted
the job. **No exit code moved**, `herd-proto` was not touched, and
nothing in `herd-runner` or `herd-hub` was touched.

**Part 1 — the peak a job reached.** `herdctl::peak_memory_line`
(`herdctl/src/lib.rs`) renders `JobMetrics::peak_memory_bytes` as one
stderr line, pushed into `said` in `herdctl/src/main.rs` so the verdict
document logs the same words a person read. It is said for every job
whose `ran_on` is `Some` — every job that reached a machine, whatever
the ending, a capped job included — and for no job that reached none,
where *not measured* would be noise about a job that never ran.

The line carries the three things the brief required of it:

1. **Its unit and the direction of its error.** The exact byte count
   through `herd_proto::size::measured_bytes`, which is the house
   renderer for a figure Herd *measured* rather than one a human typed,
   so `1073741824 bytes (1.00 GiB)`. Then, in words: a whole-job
   high-water mark over every executor step, page cache and shared
   memory counted in, so it **reads high**; and the only honest reading
   — above a declared figure is conclusive, below it is conclusive, just
   above it is conclusive of neither.
2. **Absent, never zero.** `None` prints *not measured* and carries no
   digit at all, asserted by `!absent.contains(char::is_numeric)`. A
   measured `Some(0)` still prints `0 bytes`, so the two cases cannot
   collapse into each other in either direction.
3. **Beside the declaration — it is not, and could not be from here.**
   See *Risks and findings* 2 and `ORCH-NEXT-B`.

**Part 2 — the cap's reason.** `herdctl::cap_in_words` adds one line on
exit 124 naming the cap, its limit with the unit, and the job id. `124`
is still one code for all four caps; `exit_code_for` and
`STOPPED_BY_CAP_CODE` are unchanged, and `--help`'s table still says
124 for all four. For the **memory** cap the line says the figure is the
runner's own ceiling for every job that machine accepts, set once at its
start by `--job-memory-max`, and **not** this job's declared
`--memory-mib`. That claim is made of the memory cap and of nothing
else: a declared command's `run` step may carry its own `timeout_s`
(D108), so `limit_of` returns a per-step figure for `Cap::Time`
(`herd-runner/src/job.rs:1611`) and "the runner's own, for every job"
would be false of it.

The byte limits read back through `herd_proto::show_bytes` — a figure an
operator *typed*, in the family they typed it in — beside the exact
count: `8589934592 bytes (8 GiB)`. A figure that is an exact multiple of
no unit stays in bytes once and is not printed twice.

**One thing the brief did not ask for and D30 required.** A cap carrying
a `detail` is a cap the job did **not** reach — the budget could not be
*enforced* — so `cap_in_words` takes `detail` and turns "stopped **at**
the cap of X" into "stopped **by** a cap it could not be held to", with
the reason, and stops offering the figure as a measurement. Without it
this task would have added a second copy of the exact sentence D30
exists to prevent.

**`--help`.** The 124 row now says which cap, its figure and the job's
id are on stderr, and that the memory figure is the runner's ceiling. A
`metrics` row was added to the `--verdict-to` field list saying the peak
is absent rather than zero when nobody could take it. No exit code and
no field was renamed or removed, so `VERDICT_FORMAT_VERSION` stays 1 and
`tests/frozen_verdict.rs` is untouched and green.

**Tests — six, all asserting, none skipped.** Four unit tests in
`herdctl/src/lib.rs` pin both branches of the peak line and both
branches of the cap line as exact text. Two real end-to-end assertions
run the **real binary** against a real hub and a real runner in
`herdctl/tests/exit_code.rs`: `cap:` and the figure read off the live
stderr of the log, time and disk caps, and a new test
`a_job_that_reached_a_machine_is_told_what_it_took` which asserts the
peak line is there for a job that ran, that whichever of its two
readings it carries is self-consistent, and that the sentence and
`metrics.peak_memory_bytes` in the verdict document **agree** about
whether a figure exists.

## Not done, and why

- **The record still cannot say where a peak came from.** `herdctl`
  cannot print *sampled, and it undercounts* because nothing in
  `JobMetrics` carries that: `herd-runner/src/job.rs:593`-`:596` is
  `JobCgroup::peak_bytes().or(ran.peak_rss)` and the branch that won is
  dropped. So the line names both possibilities. The fix is a
  `herd-proto` change, which the brief forbade and which is Alexa's
  call; filed as `ORCH-NEXT-A`.
- **The declaration is not printed beside the peak**, because it does
  not reach the point the verdict is composed. `ORCH-NEXT-B`.
- **`deploy/release-notice.md` was not edited.** Its `124` row at
  `:378` already says *"Which cap it was is printed on stderr and
  carried in the record"*, which was true before this task and is more
  true after it, so nothing there is false. The memory-ceiling caveat
  would be worth having in that table too, and that is a prose task
  outside this one's write paths.
- **`docs/status.md` was not touched at all** — it is at 90 lines
  (D276), and grepping it for `peak`, `124` and `cap` found no pointer
  this task falsifies, dead or otherwise. **`docs/coverage.md` was not
  touched**: another branch holds it, and its E-15 note was read and
  says nothing about what `herdctl` displays, so this task falsifies
  nothing in it.
- **No pull request, no merge, no tag, nothing on `main`**, and
  `docs/decisions.md` was not opened — the entry this task owes is
  drafted below as `D-NEXT`, with no number taken.

## Gates

**All four Rust gates plus `check-docs.sh`, through Herd as one bare
`./scripts/gate.sh`** — this change is `.rs`, so the prose-only
exemption does not reach it. Our own instance,
`/mnt/ssd_980/herd/queue/bin/herdctl --hub ws://127.0.0.1:8791`, never a
bare `herdctl` (D161, invariant 9), with `--label build`,
`--memory-mib 4700`, and a submitted command carrying its own absolute
`cd` into this worktree and its own `CARGO_TARGET_DIR` under
`/mnt/ssd_980/herd/build/`. `./scripts/bench-preflight.sh` was run
before every heavy phase and **refused once**, at load 9.35 against a
9.00 ceiling; that phase waited 250 s and started when it cleared.

**Two gate runs, and the first was RED.** Both are reported; the red one
is not hidden behind the green one.

- **`j0000000173` on `671b6c13dc114253bac35d3e064f458e0de573e1` — RED.**
  Steps 0 to 5 and 7 all ok — `check-docs.sh`,
  `check-orchestration.sh`, `nix flake check`, `cargo fmt --all
  --check`, `cargo clippy --workspace --all-targets -- -D warnings` —
  and step 6, the suite, failed on **one** assertion out of the whole
  workspace:

  ```text
  test herdctl_exits_124_for_every_cap_a_test_can_trip ... FAILED
  test result: FAILED. 14 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.51s
  ```

  ```text
  gate: RED sha=671b6c13dc114253bac35d3e064f458e0de573e1 failed=6:test-suite skips=under-load(flood-fifty=9.34>8,flood-two-fifty=9.34>8)
  gate: NOT GREEN — this sha must not be pushed to main.
  ```

  **It was an assertion written by this task, on a product that was
  right**, and the fix is in the assertion and nowhere else. The whole
  verbatim panic and the two controls that settle it are in *Risks and
  findings* 1.

- **`j0000000176` on `bb242f7a26a9990d6592d96260d7ae2d10d2faa3` —
  GREEN**, `"passed": true`, `"exit_code": 0`, `ran on: queue-local`.
  Its own last line, verbatim:

  ```text
  gate: GREEN sha=bb242f7a26a9990d6592d96260d7ae2d10d2faa3 steps=0-8:ok host=servernixos load1=4.90 load1-end=7.62 skips=under-load(flood-fifty=8.77>8,flood-two-fifty=8.77>8)
  ```

  Receipt: `<worktree>/target/herd-gate/<sha>.receipt`, where `<sha>` is
  the full forty above; started `2026-10-31T11:59:58Z`, finished
  `2026-10-31T12:06:55Z`. The absolute path is in the agent's report.

**Skips — 14 runtime plus 38 `#[ignore]`, each by name, none counted as
a pass (D27).** Step 7 said *"the skip list matches the expectation
exactly"* against `scripts/expected-skips/local.txt`, and the two that
are not on the at-rest list were excused **by name** out of
`scripts/expected-skips/local-under-load.txt`, each out of the bench's
own reading and not out of a figure the gate took:

```text
    excused  flood-fifty: the bench itself read 8.77>8, the ceiling
             being the one its own skip line names, and declined
    excused  flood-two-fifty: the bench itself read 8.77>8, the ceiling
             being the one its own skip line names, and declined
```

That is the **under-load list** — twelve at rest plus those two — which
is the mechanism working, compared as a list and never as a count
(D187). The `#[ignore]` census matched its expectation exactly and every
one carries a reason. **This task added no skip and no `#[ignore]`.**

**The last run, named and not quoted:** `./scripts/check-docs.sh` on the
tree with this file committed, run locally because it needs no
toolchain. Its output goes to the agent's report and to nothing in this
tree. The two Rust runs above are the runs made **before** this file was
committed; the gate they name tested `bb242f7`, whose tree differs from
the pushed tree only by this file.

## Risks and findings

1. **The red gate was mine, and it was not load.** The assertion that
   failed was one this task added, and it asserted one branch of a
   two-branch behaviour as if it were the only branch. Verbatim, from
   `j0000000173`'s suite output:

   ```text
   thread 'herdctl_exits_124_for_every_cap_a_test_can_trip' (3201812) panicked at herdctl/tests/exit_code.rs:790:5:
   a cap that could not be enforced must not read as one the job reached: herdctl: submitting at priority step (the default; --priority takes one of merger, step, nightly, producer)
   herdctl: job-id: j0000000001
   herdctl: this job has no verdict of its own: the runner stopped this job at its disk cap of 8388608 bytes
   herdctl: cap: job j0000000001 was stopped at the disk cap of 8388608 bytes (8 MiB), and at no other cap.
   herdctl: this job's peak memory was not measured: no figure for it came back, which is not the same thing as a job that held no memory and must not be read as a zero
   ```

   That job writes 40 MiB against an 8 MiB budget and only *then* makes
   its directory unreadable, so either end can come first: the 200 ms
   sampler seeing the tree go over the budget, which is a cap the job
   **reached** and carries no `detail`; or the final walk after the job
   exits finding `h/` unreadable, which is a cap that could not be
   **enforced** and carries one. The assertion demanded the second.

   **Two controls, named as controls, both local, both after the gate
   and never beside it.** (a) The test alone on `bb242f7`:
   `test result: ok. 1 passed; 0 failed`. (b) The decisive one — the
   **old** assertion restored from `671b6c1` and run alone at load1
   **8.69**, under the 9.00 ceiling: it **failed again**, with the same
   sentence and the same wording. So **the load hypothesis is ruled out
   and not assumed**: this is not a test that fails only under load, it
   is a test that was wrong about which path this host takes, and this
   host takes the sampler path at rest as well as at load 9.34. The
   worktree was returned to `bb242f7` and `git status --porcelain` is
   empty.

   **What the fix does and does not do.** It asserts what D30 actually
   requires — that the two sentences never read alike — in both
   branches, instead of asserting which branch fires. Every assertion
   the test already had is untouched: the 124, the `"disk cap"`
   substring, the 1024-byte log and the time cap. Nothing was relaxed
   to accommodate this change, and no test this task did not write was
   edited.

2. **The peak cannot be put beside the declaration from the requester's
   side, and this is a premise of the brief that the code refutes.**
   `FromHub::JobFinished` (`herd-proto/src/lib.rs:1635`) carries the
   outcome, the attempt, the chunk count, the metrics, the steps and the
   outputs, plus the commit — and **none of the job's requirements**.
   Both `Finished` construction sites in `herdctl/src/lib.rs` build from
   that frame alone. `herdctl run` does hold the declaration in its own
   process, but `herdctl wait <id>` never knew it, so threading it
   through `herdctl` alone would print the pair on one subcommand and
   not the other — worse than printing it on neither. So the brief's
   fallback was taken: the peak goes out alone and says what it is.
   `ORCH-NEXT-B` carries what would be needed.

3. **A second premise of the brief, corrected rather than worked
   around: the peak was already in the verdict document.** The brief
   says the figure *"appears nowhere in `herdctl/src`"*, which is true
   of the identifier and was verified — `grep` over `herdctl/` returned
   it only in `tests/frozen_verdict.rs`. But `Verdict::metrics` is
   `Option<JobMetrics>` and `Verdict::of` fills it with
   `finished.metrics.clone()`, so `peak_memory_bytes` has been in every
   `--verdict-to` document all along; `tests/frozen_verdict.rs:170`
   froze it as `"peak_memory_bytes": 1073741824` before this task
   existed. **So nothing was added to the document**, which would have
   meant two copies of one figure in a frozen public format. What this
   task added there is the *reading* — `Verdict::metrics`'s doc comment
   now carries the unit, the direction of the error, the absent-never-
   zero rule and the missing declaration — plus the sentence itself in
   `said`, which a wrapper reads as the words a person read.

4. **ORCH-268's figure measured twice more, by this task's own gate
   jobs, and it is worse than a defect in a doc comment.** Both gate
   runs compiled the entire workspace and ran the whole suite, and both
   recorded a peak of about **3.2 MB**: `peak_memory_bytes: 3235840` on
   `j0000000173` and `3260416` on `j0000000176`, beside
   `bytes_written: 20487811719` and `wall_time_ms: 417497`. The reason
   is in this host's own runner stderr, captured verbatim during the
   control:

   ```text
   herd-runner: memory ceiling: FALLING BACK to a per-process address-space limit, which bounds each process instead of the job's tree and makes an allocation fail instead of killing the job. Reason: cgroup v2 delegation is unavailable: this runner cannot create a cgroup under /sys/fs/cgroup/chatons.slice/chatons-herd-claude-256608.scope (Permission denied (os error 13)). Run it as a systemd unit with Delegate=yes, or inside a delegated scope, and point --cgroup-root inside that subtree.
   ```

   So on this host the figure is **always** the one-pid sample, and it
   arrives in the record shaped exactly like a cgroup measurement. The
   new line hedges between the two provenances because the record gives
   it nothing to choose with; `ORCH-NEXT-A` is the row that closes it.

5. **A board edit landed inside gate run 1 and affected no step of
   it — measured, not assumed.** `docs/orchestration/board.md` was
   edited at `11:47:04Z` while `j0000000173` was at step 6. Gate 1's
   step 0 asserted the tree clean at `11:44:41Z`, step 1
   (`check-docs.sh`) ran at `11:44:42Z` and step 2 finished about
   `11:45:42Z` — all before the edit — and steps 4 to 6 read `.rs` and
   fixtures. `gate.sh` checks cleanliness at step 0 only. **It does not
   matter to what is being handed over either way**, because the
   authoritative run is `j0000000176`, whose own step 0 says *"the
   working tree is clean: this gate is about
   `bb242f7a26a9990d6592d96260d7ae2d10d2faa3` and nothing else"* with
   the board already committed.

6. **The `said` list grew by one or two sentences for a job that ran.**
   That is deliberate — a wrapper reading the document logs what a
   person read (D166) — and it moves no version, because `said` is
   prose nothing is meant to match on and `VERDICT_FORMAT_VERSION`
   tracks fields. `tests/frozen_verdict.rs` supplies `said` as an input
   and is unaffected, which was verified by its being green in both gate
   runs.

7. **Not verified by a live terminal: the *present* branch of the peak
   line.** This host delegates no cgroup, so every job a local test runs
   reads *not measured*, and that is the branch the end-to-end test
   exercised and the branch quoted verbatim above. The present branch is
   pinned by a unit test on exact substrings — `1073741824 bytes`,
   `1.00 GiB`, `whole job`, `page cache`, `reads high`, and the three
   clauses of the conclusion — and the sentence quoted in the agent's
   report for it is **rendered from the format string, not captured from
   a terminal**, and is labelled as such there.

## Drafted for `herd-merger`

### The `docs/decisions.md` entry, in full

```markdown
## 2026-10-31 — D-NEXT A requester sees what its job took, and rc 124 says which cap

**Context.** Herd measured two things and showed the requester neither.
`JobMetrics::peak_memory_bytes` has been read from the job cgroup's
`memory.peak` since M4 and kept in every job's record, and `herdctl`
printed it nowhere: a requester that wanted to know what its build held
had to go and read the record. And `rc 124` is one code for all four
caps by design, with the cap only in the outcome — so nobody could learn
from the exit whether it was time, memory, log or disk. The first
consumer hit the sharper half of that on 2026-10-30: it read a memory
cap of `8589934592 bytes` against a job that had declared 1.7 GiB, and
took the figure for its own declaration. It is not. There is exactly one
`memory.max` write in the product and the number it writes is built once
at the runner's start from `--job-memory-max`; a requester's
`--memory-mib` is a placement requirement (E-29) and never a ceiling.

**Choice.** Alexa, 2026-10-31, two decisions taken together. A requester
must be able to see what its job actually took. And **`rc 124` stays one
code for all four caps**, with the reason made legible instead of being
split into per-cap codes. So `herdctl` says both in words, on stderr and
in the verdict document's `said`: the peak, with its unit and the
direction of its error, and which cap stopped the job at what figure
with the job's id — the memory figure named as the runner's own ceiling
for every job it accepts and not as the job's declaration. **No exit
code moves and no protocol field moves**: `VERDICT_FORMAT_VERSION` stays
1 and the figure was already in the document under `metrics`, so what
was added is the reading and the sentence, not a second copy of the
number.

Three things the peak figure has to carry wherever it is read, because a
figure quoted without them is a figure somebody tightens a declaration
on. **Its direction of error**: `memory.peak` is a whole-job high-water
mark over every executor step and counts page cache and shared memory,
so it reads **high** — a peak above a declared figure is conclusive that
the job over-ran it, a peak below it is conclusive that it did not, and
a peak just above it is conclusive of neither. **Absent, never zero**
(D23): a figure nobody could take prints *not measured* and carries no
digit, because a small number is what the broken case looks like — about
3.2 MB for a job that compiled an entire workspace (ORCH-268), measured
again twice on 2026-10-31 by this change's own gate jobs. **And it goes
out alone**: `FromHub::JobFinished` carries no requirement of the job,
so the declaration a peak would have to be read against is not there,
and the line says what it is instead of implying a comparison nobody
made.

**Alternative rejected.** Splitting 124 into four codes, one per cap.
Refused: the four caps are one event — the machine said no, so the job
has no verdict of its own — and a requester branching on four numbers
would be branching on which resource ran out rather than on whether it
has an answer at all, which is the distinction D20 exists to protect.
Also rejected: adding a second copy of `peak_memory_bytes` as a field of
its own on the verdict document, which is a frozen public format where
two copies of one figure is a reader's ambiguity; and printing a zero or
the largest figure the runner happened to see in place of an absence,
which is ORCH-268's defect rather than a stopgap for it.
```

### The `board.md` rows

Both are **already written into `docs/orchestration/board.md` on this
branch**, at the end of *Owed*, as `ORCH-NEXT-A` and `ORCH-NEXT-B`; the
merger allocates the two numbers. The same commit also appends to
**ORCH-268**'s *Next action* cell, recording that its display half is
done and that the row does not close, and pointing at the two new rows
by their provisional ids. Those three ids are the only cells to
renumber, and `ORCH-268`'s cell references `ORCH-NEXT-A` and
`ORCH-NEXT-B` by name, so the rename has to reach it too.

## Decisions needed

**Question.** Should `herd_proto::JobMetrics` carry how each resource
figure was obtained — a cgroup read, a sampled read, or absent — so that
a one-pid sample stops arriving shaped like a measurement?
**Recommendation:** yes, and it is `ORCH-NEXT-A`; but it is a protocol
surface, so it is Alexa's, and ORCH-268 is where that question already
sits with him — nothing should start before he answers. **Fallback:**
leave the record as it is and keep the hedged sentence this task wrote,
which is honest about not knowing and is strictly better than the
silence it replaces, at the cost of a requester who can never tell a
3.2 MB sample from a 3.2 MB job.

## Resume point

Nothing is in flight. `./scripts/check-docs.sh` is the one run left and
it is this task's last act, before the push. After that, the branch is
ready for `herd-merger` to take into a batch: it needs the `D-NEXT`
number allocated, the two `ORCH-NEXT-*` rows numbered in three places
(both new rows and ORCH-268's cell), and nothing else. The next piece of
work is `ORCH-NEXT-A`, which cannot start until Alexa has answered
ORCH-268.

## Processes

PIDs started: two `herdctl` clients of our own queue instance, one per
gate run, plus the local `nix develop` control runs. **Nothing was
signalled and nothing was killed**: no `pkill -f` in any form, no kill
by name, and the four untouchable PIDs (3006543, 3021002, 565152,
662711) were never addressed. uid 1002 is shared with the consumer's
production, and the one process listing this task took was read for its
full `args` before anything else was done.

Two Herd jobs: **`j0000000173`** (RED, reported red) and
**`j0000000176`** (GREEN). Both through
`/mnt/ssd_980/herd/queue/bin/herdctl --hub ws://127.0.0.1:8791`, never a
bare `herdctl`. Neither was re-run: `j0000000176` is a **different sha**
after a real fix, not a resubmission of `671b6c1` — a gate that gave a
verdict is not retried.

Own build tree deleted once the branch was pushed:
`/mnt/ssd_980/herd/build/herdctl-peak-and-cap-legibility`. Its size at
removal is in the agent's report. The worktree's own `target/` is kept
only because it holds the gate receipt, the gate logs and the suite
output for `bb242f7`; it holds no build output, because
`CARGO_TARGET_DIR` pointed off it for every compile.
