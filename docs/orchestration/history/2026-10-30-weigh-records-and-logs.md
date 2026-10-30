# weigh-records-and-logs — handoff 2026-10-30

**The condition v1.0.0 ships on was not met, and now is.** The first
consumer agreed on 2026-10-30 that v1.0.0 may ship without the hub's
record retention, following in v1.1, on two conditions. The second, in
their words: *"`herd-hub report` continue de montrer le poids des
enregistrements et des journaux, pour que nous voyions la croissance au
lieu de la découvrir."* Before this branch, `herd-hub report` weighed the
**collected output** in sections 1 to 3 and `packs/` in section 4, and not
one of its five figures weighed a `job.json`, an `events.jsonl` or a
`log`. Section 3's reconciliation is a record's *claim* against what
`files/` measures, which is the output again. Measured on our own live
state root, the gap this leaves is total and not marginal: that root holds
**1,443,667 bytes** of bookkeeping and **0 bytes** of collected output, so
every figure the report printed about it was nought while the root was
1.38 MiB heavy. Section 5 is that weight.

- **Item:** none allocated — `ORCH-282` on `main` is the `workflow.md`
  bullet and is a different thing; a row is drafted below
- **Agent:** herd-step
- **Branch / commit:** `orch-282-weigh-records` /
  `92b9f07` (section 5), then the apparent-size paragraph, then this
  file, based on origin/main `df47266`. The branch was pushed once
  before the paragraph existed, so `origin` already holds `92b9f07` and
  `a34ec79`; this push adds the two commits above them and nothing is
  rewritten.
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

**`herd-hub report` prints a fifth section, `5. the records, the events
and the logs`.** Over **every** job directory under `jobs/`, it weighs by
`metadata().len()`:

- the `job.json` records, with a count of the directories holding one;
- the `events.jsonl` files, with their count;
- the `log` files, with their count — or named as `UNKNOWN`;
- `log.seq` and anything else beside them, as one catch-all;
- the heaviest single directory, by name;
- the collected output beside it (`Accounting::collected_bytes()`), and
  the bookkeeping's share of the two.

**The two totals partition a job directory, and that is asserted.** The
three collection directories are skipped **by name** inside
`weigh_one_directory`, never subtracted from a total, so no byte can be in
both figures. Two tests assert the partition against a walk the test does
itself rather than through the code under test: on a hand-built fixture,
and on a root the store wrote, in
`herd-hub/tests/state_root_report.rs`,
`the_report_reads_what_the_store_itself_wrote`.

**It is a total, and it says so.** The printed prose says why — a mean
over these directories would misread one the way section 2 says a mean
misreads a job, because a single large log can be the whole of the
difference — and the **heaviest single directory** is printed beside the
total so that a total cannot pass for an even root. No mean, anywhere, and
no decile: a decile table would answer a diagnostic question, and the
condition is about watching a total grow.

**It prints its own unit, and the direction of its own error.** The
printed prose says the figure is apparent size summed from file lengths,
that apparent size is a **floor** on what the files occupy because a
one-byte file costs a whole block and a bookkeeping population is almost
all small files, gives the one measurement behind that — 1443667 bytes
apparent against 2850816 bytes of blocks on a real root, about 1.97× —
says the factor is a property of that root's file sizes and block size
and not a constant to apply elsewhere, and ends with the instruction:
compare `du` with `du`, or apparent with apparent, never one against the
other. It is in the **output** and not in a comment because the
consumer's own ≈ 346 MiB for this quantity is `du -sk`, so blocks, and
section 5 on a comparable root prints about half of it with nothing
having shrunk.

**A log that could not be measured is `UNKNOWN`, never nought,** and the
total then prints `A FLOOR AND NOT A TOTAL` with the count. This reuses
`crate::retention::LogWeight` rather than spelling the same enum again:
the rule it carries is one rule, and a second copy would be free to drift
from it.

**D257 is honoured.** A root with no job directory at all, and a root
whose directories hold no `job.json` yet, print two different banners
rather than a bare nought — the reading "the bookkeeping weighs nothing"
is said to be impossible, because a record is never nought bytes.

**Every job directory is weighed, read or not.** Sections 1 to 3 stop at a
record this build cannot parse. Its bytes are on the disk either way, so
section 5 weighs it and prints `job directories weighed` (the census's
`directories`) rather than `records read`, with the asymmetry stated in
the output itself.

**Section 4 became a function.** `write_packs` is extracted because its
`return Ok(())` for a root with no `packs/` was harmless only while
section 4 was last. Section 5 follows it, so on every root a test builds
by hand the new figures would have silently vanished. The integration
test's whole-text assertion is the guard: that fixture has no `packs/`.

**The three guarantees in the brief all still hold, and the two header
sentences were amended rather than quietly weakened.** No lock is taken
(no new lock call, no store built); each record is still read whole or not
at all; no output byte, no log byte and no line of `events.jsonl` is read
— only lengths. `report` remains read-only: no new write, no removal. The
module header and the report's own header now say *"the measurements in
sections 3 to 5 are file lengths, a log's own length among them"* in place
of *"section 3's measurements are file lengths"*, which was already stale
for section 4.

**Measured on a frozen copy of our own state root, every figure
reconciles to the byte** against an independent sum taken with
`find -printf '%s'` (a different tool from the report's `metadata().len()`
and from `du`):

| class | report | `find -printf '%s'` |
|---|---|---|
| `job.json` | 141693 bytes in 112 | 141693 bytes in 112 |
| `events.jsonl` | 130246 bytes in 112 | 130246 bytes in 112 |
| `log` | 1167024 bytes in 112 | 1167024 bytes in 112 |
| anything else (`log.seq`) | 4704 bytes | 4704 bytes |
| in all | 1443667 bytes | 1443667 bytes |
| heaviest directory | 158500, `jobs/j0000000088` | 158500, `jobs/j0000000088` |

Files touched: `herd-hub/src/report.rs`, `herd-hub/src/main.rs` (the
subcommand's doc comment, which said *"Three figures"*),
`herd-hub/tests/state_root_report.rs`, `docs/operating.md`. The two prose
files were already stale for section 4 and are now right for five.

## Not done, and why

- **`status.json` carries no bookkeeping figure.** `herd-hub/src/status.rs`
  was named in the brief as the one file not to touch — `STATUS_VERSION`
  moved to 2 this week and two unlanded branches want changes there. A row
  is drafted below so the figure rides with them and the version moves
  once.
- **No deciles of per-directory bookkeeping.** A total with its maximum is
  what the condition asks for; a distribution is a diagnostic and would be
  a second decision about what the report is for. If it is wanted, it
  should reuse `summarise`/`Sizes`, whose `naming_nothing` field would need
  its doc generalised first.
- **`Bookkeeping` is not re-exported at the crate root.** Every other
  `report` type is in `lib.rs`'s `pub use report::{…}`. It is reachable as
  `herd_hub::report::Bookkeeping` and nothing needs the shorter path, so
  `lib.rs` — a high-traffic file with unlanded branches on it — was left
  alone. A one-line follow-up, in the drafted row.
- **Nothing outside a job directory is weighed.** `store.json`,
  `index/state.json`, `readings/answers.json` and `hub.lock` are in no
  figure of any section. They are small and fixed-count, the brief named
  the records, the logs and the event files, and section 5's own output
  says *"Nothing under `packs/` is in either"* — but a reader adding the
  two printed totals does not get the whole root. Noted as a finding.

## Gates

**All five ran as Herd jobs**, one job per gate, through
`/mnt/ssd_980/herd/queue/bin/herdctl` with `--label build
--memory-mib 4700`, each against this worktree with
`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/orch-282-weigh-records`.
`./scripts/bench-preflight.sh` was run before every heavy phase and
**refused once** (8035 MiB under `memory.high` against a floor of 8192);
the phase waited for it to clear and
`HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB` was never set.

**They ran twice, and the second round is the evidence.** The
apparent-size paragraph is *printed text*, so adding it made this product
code again and all four Rust gates were re-run on the tree that carries
it. The build tree had already been deleted after the first push, so the
second round compiled from cold. Quoted from the second round, made
before this file was committed:

```text
cargo build --workspace
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 27.71s

cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 23.53s
    (the one line matching "warning" is nix's "Git tree ... is dirty")

cargo fmt --all --check
fmt: clean (round 2)

./scripts/test.sh
ignored census: COMPLETE (38 ignored, 0 without a reason)
test suite: PASSED
```

The first round, before the paragraph, was green on the same five gates:
build 27.21s, clippy 18.59s, `fmt: clean`, suite PASSED with the same
census, `docs ok`.

**One expectation was changed, and it was changed because the text
changed on purpose.** `herd-hub/tests/state_root_report.rs`'s byte-exact
whole-text assertion went red on the new paragraph, as it should; the
paragraph was reviewed line by line and the expectation edited to match,
not regenerated from the failing output. The reason is written into
`EXPECTED`'s own doc comment so that a later reader can see the text was
reviewed rather than pasted back.

**Skips: 12, and the list is what was compared** (D187 — the count is
what the list is read off, and the two rounds' lists were `diff`ed name
for name and are identical; so is the 38-name `#[ignore]` list). All
twelve are the host's shape and not this change: eleven are
`cgroup v2 delegation is unavailable … Permission denied` under
`/sys/fs/cgroup/chatons.slice`, and one is
`a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device`,
which refused an `io.max` on device 259:0 for the same reason. The
`#[ignore]` census is **COMPLETE at 38**. Not one skip is in `herd-hub`
and not one is in this change's tests.

**The last run, named and not quoted**, because this file is itself a docs
file and the run before it gated a different tree:
`./scripts/check-docs.sh`, as a Herd job on the committed tree, its output
to `scratchpad/orch-282/r2-gate5-docs-final.txt` and verbatim in the
agent's report. The Rust gates are **not** re-run after this file, which
changes no code: the four above were run on the tree this file is
committed on top of.

## Risks and findings

1. **The report's figure is apparent size and is a *floor* on real disk
   here, in the opposite direction from `bytes_written`'s trap.** Measured
   on the frozen copy with two different tools, per the gotcha
   `bytes-written-is-the-workspace-not-the-job.md`: 1443667 bytes apparent,
   **2850816 bytes** of real blocks (`find -printf '%b'`, 5568 blocks ×
   512), and **3772416 bytes** by `du -s --block-size=1` of `jobs/`
   (which also counts directory inodes). About **1.97×** the apparent
   figure, because 448 small files each cost at least one block. So when
   the consumer reads "346 MiB of bookkeeping", the disk cost is *higher*,
   not lower — the reverse of a sparse file. No figure was checked with
   `du -b` or `--apparent-size` anywhere in this task.

   **This is now in section 5's printed prose and not only here, and the
   reason is measured.** The consumer's own ≈ 346 MiB for this very
   quantity is `du -sk` *without* `--apparent-size`, so allocated blocks.
   Section 5 sums `metadata().len()`, so apparent size. **At their
   migration section 5 will therefore print roughly half of what their
   `du` told them, on a comparable root, with nothing having shrunk** —
   and the natural reading of that is "records got smaller", from which
   somebody concludes retention is less urgent than it is. The
   orchestrator made the mirror-image error on 2026-10-30 with both
   measurements in front of it, taking the correct block figure and
   "correcting" it upward by this 1.97×. So the output now carries the
   unit, the direction of the error, the single measurement behind the
   factor, the caveat that the factor is a property of that root's file
   sizes and block size and not a constant, and the instruction: compare
   `du` with `du`, or apparent with apparent, never one against the other.

   **The pairing, which is the sharper point.** This product now has
   **two apparent-size figures that err in opposite directions**. A job's
   `bytes_written` (**D263**, ORCH-278) **overstates**: a sparse file
   counts for its holes, and that killed a landing's gate the same day
   (`gotchas/bytes-written-is-the-workspace-not-the-job.md`). Section 5
   **understates**: apparent size floors real occupancy. Same unit, same
   word, opposite bias, and nothing in the product says which is which.
   That is a reason of its own to separate a **workspace quota in
   apparent size** from **bytes written as I/O** — ORCH-278's question —
   because one name cannot carry two biases, and the drafted board row
   below says so rather than leaving the two findings in two files.
2. **A live root does not reconcile, and should not.** The same run on the
   *live* queue root differed from a `find` taken a minute later by 2938
   bytes across three classes, all growth while jobs ran. That is the
   header's own *"the figures together are not one instant"*, now observed
   rather than asserted. Any later reconciliation must be done on a frozen
   copy.
3. **Nothing here touches a cap or a bound.** `bytes_written` (D263,
   ORCH-278) was not read, not redefined and not referenced. Section 5
   adds a reading; it enforces nothing, and the report still removes
   nothing and serves nothing.
4. **The brief's premise was correct in full** and no measurement refuted
   it: `herd-hub report` at `df47266` weighs no record, no event file and
   no log, and `retention.rs` was the only place in the hub that did. That
   adjacency is the finding worth carrying: `herd-hub retention-preview`
   has weighed all three per record since ORCH-168, so the condition's
   figure existed in the binary and not in the command an operator was
   told to run. Section 5 now states it in `report`, and reuses
   `retention::LogWeight` so the two cannot disagree about an unmeasured
   log.
5. **Two measurement rules are now duplicated across the two modules** —
   `EVENTS_FILE`/`LOG_FILE`/`LOG_SEQ_FILE` are written out in `report.rs`
   as `retention.rs` already writes two of them, because they are private
   to `store.rs`. Both copies are held by tests that drive the store's own
   writer, which is the house rule, but a third reader will write them a
   third time. Widening them in `store.rs` is one line and belongs to
   whoever owns that file.

## Drafted for `herd-merger`

### `docs/decisions.md` — the full entry, headed `D-NEXT`

```markdown
## 2026-10-30 — D-NEXT A report weighs the bookkeeping, not only the output

`herd-hub report` had four figures and every one of them was about the
**collected output** or about `packs/`: the deduplication ratio over what
records claim, per-job output size by decile, what records claim against
what `files/` measures, and what `packs/` holds. Not one weighed a
`job.json`, an `events.jsonl` or a `log`.

**That made the report blind in the one direction the first consumer
depends on.** They agreed on 2026-10-30 that v1.0.0 may ship without the
hub's record retention, following in v1.1, on two conditions, the second
being that `herd-hub report` keep showing the weight of the records and
the logs *"so that we see the growth instead of discovering it"*. Their
own root: about 346 MiB over 3334 jobs, 0.32 %, against 105.57 GiB of
collected files, 99.68 %. Our own queue root is the limit case and was
measured, frozen, on the same day: **1,443,667 bytes** of bookkeeping
against **0 bytes** of collected output. Every figure the report printed
about that root was nought.

**So a report over a population nothing bounds prints that population.**
Section 5 weighs the records, the event files and the logs of every job
directory, with their counts, beside section 3's collected figure. Three
things are settled with it and each is a rule and not a detail.

1. **The two totals partition a job directory.** `files/`, `files.part/`
   and `files.gone/` are skipped **by name**, never subtracted from a
   total, and a test asserts that the two sums come to every byte under
   `jobs/` — walked by the test and not by the code under test, so a byte
   counted twice and a byte counted nowhere redden the same assertion.
2. **A total, said to be one, with the maximum beside it.** Deciles are
   section 2's answer because a per-job output size is skewed by what a
   job kept; the question here is what the bookkeeping weighs, which is a
   sum. A mean is still refused — one 64 MiB log is the whole of the
   difference between two roots — so the heaviest single directory is
   printed beside the total.
3. **An absence is never a nought** (D257, one level down from a version
   number). A root with no job directory and a root whose first records
   have not landed print two different banners, and the reading "the
   bookkeeping weighs nothing" is said to be impossible, because a record
   is never nought bytes. A log that could not be measured is `UNKNOWN`,
   the total says it is a floor, and the enum that carries that rule is
   `retention::LogWeight` reused rather than spelled a second time.

**What the figure is, exactly, so nobody quotes it as something else.** It
is apparent size, `metadata().len()` per file, like every other byte figure
this report prints. Measured the same day with two different tools on a
frozen copy of our own root, real occupancy is **1.97× higher** — 2850816
bytes of blocks against 1443667 apparent, because small files each cost a
block. **So the printed prose carries the unit, that one measurement, the
caveat that the factor is a property of that root's file sizes and block
size and not a constant, and the instruction that compares `du` with `du`
and never one unit against the other.** It is in the output and not in a
comment because the consumer's own figure for the same quantity is `du
-sk`, which is blocks: at their migration section 5 prints about half of
what their `du` said with nothing having shrunk, and the natural reading
of a root halving is that retention got less urgent. The orchestrator
made the mirror-image error the same day with both measurements in front
of it.

**And the pairing, which is why the unit has to be printed and not merely
known.** This product now holds **two apparent-size figures that err in
opposite directions**: a job's `bytes_written` (D263, ORCH-278)
**overstates**, because a sparse file counts for its holes, and it killed
a landing's gate on 2026-10-30; this one **understates**, because
apparent size floors real occupancy. Same unit, same word, opposite bias.
One name cannot carry two biases, which is an argument of its own for
ORCH-278's separation of a workspace quota in apparent size from bytes
written as I/O.

Reversible: it is a read. `report` takes no lock, writes nothing, removes
nothing, and reads no output byte, no log byte and no line of an event
file — only lengths. The cost is one `read_dir` per job directory that the
walk already visits.
```

### `docs/orchestration/board.md` — one row, for *Owed*

Columns: `ID | Item | Origin | Owner | State | Next action | Depends on |
Evidence`. The merger allocates the ID (the highest on `main` is
ORCH-282); no literal pipe appears in any cell.

```markdown
| ORCH-NEXT | **The bookkeeping weight is in `herd-hub report` and not in `status.json`, so a watcher polling the hub cannot see the growth the consumer asked to watch.** Section 5 landed on this branch and prints the records, the event files and the logs of every job directory with their counts. `status.json` carries none of it, so the figure reaches a reader only by somebody running `herd-hub report` in the hub's container and pasting the output. `herd-hub/src/status.rs` was deliberately not touched: `STATUS_VERSION` moved 1 to 2 this week and two unlanded branches already want changes there, so a third would move it twice for one release | the `weigh-records-and-logs` brief's own instruction, 2026-10-30, and its handoff *Not done* 1 | herd-step | owed | Add the bookkeeping total and its three class figures to `status.json` **in the same change as the other two branches' fields**, so `STATUS_VERSION` moves once. Reuse `report::Bookkeeping` rather than measuring again; an absent field must stay distinguishable from a hub that does not record it (D257). While doing it, re-export `Bookkeeping` from `herd-hub/src/lib.rs` beside the other `report` types, and consider widening `EVENTS_FILE`, `LOG_FILE` and `LOG_SEQ_FILE` in `store.rs` to `pub(crate)` so `report.rs` and `retention.rs` stop writing them out | the two unlanded branches that want `status.rs`; whichever lands first | `herd-hub/src/report.rs` section 5; `herd-hub/src/status.rs` `STATUS_VERSION`; `herd-hub/src/retention.rs` `Weight` and `LogWeight`; `history/2026-10-30-weigh-records-and-logs.md`; **D257**; the drafted `D-NEXT` of that handoff |
```

### `docs/orchestration/board.md` — a second row, for *Owed*

The pairing of the two apparent-size figures. A **second row** and not an
edit of ORCH-278's, because ORCH-278 is Alexa's open question and an
agent does not rewrite its text; this row is the measured input that
arrived after it was filed, and it names it.

```markdown
| ORCH-NEXT | **Herd now reports two apparent-size byte figures whose errors point in opposite directions, and nothing in the product says which is which.** A job's `bytes_written` **overstates**: it sums `metadata().len()`, so a sparse file counts for its holes, and on 2026-10-30 a 30 GiB declaration occupying nought blocks turned a landing's gate into `stopped_by_cap / disk` (D263). `herd-hub report` section 5, new on branch `orch-282-weigh-records`, **understates**: it sums the same quantity over a population of almost all small files, where apparent size is a floor on occupancy — 1443667 bytes apparent against 2850816 in blocks on our own queue root, about 1.97 times, measured the same day with `find -printf '%b'` against `find -printf '%s'`. One word, one unit, two biases. Section 5 prints its own unit and its own direction in its output; `bytes_written` prints neither | the `weigh-records-and-logs` handoff, 2026-10-30, *Risks and findings* 1, and the orchestrator's own mirror-image error reading the consumer's `du -sk` as apparent size that day | herd-step | owed | Carry this to **ORCH-278** as measured input rather than opening a third question: it is a reason of its own for separating a **workspace quota in apparent size** from **bytes written as I/O**, since a single name cannot carry a figure that overstates and a figure that understates. Until that is answered, any new byte figure prints its unit and the direction of its error, the way section 5 does. **Change no cap and no bound** under this row | **ORCH-278** and **D263**, which own what a cap enforces | `gotchas/bytes-written-is-the-workspace-not-the-job.md`; `herd-hub/src/report.rs` section 5 and its printed apparent-size paragraph; `herd-runner/src/root.rs` `Workspace::measure`; `history/2026-10-30-weigh-records-and-logs.md`; **D263**; **ORCH-278** |
```

## Decisions needed

none. The brief named the condition, the shape and the owner; nothing in
it was refuted and nothing structural was decided here.

## Resume point

`git fetch && git log --oneline origin/orch-282-weigh-records -1` should
show this file's commit on top of `92b9f07`. The branch is ready to be
taken into a batch integration by `herd-merger`; it touches
`herd-hub/src/report.rs`, `herd-hub/src/main.rs`,
`herd-hub/tests/state_root_report.rs` and `docs/operating.md`, and
**conflicts with nothing in `herd-hub/src/status.rs`**, which it does not
touch. To see the figure on a real root:

```sh
nix develop --command cargo run -q -p herd-hub --bin herd-hub -- \
  report /mnt/ssd_980/herd/queue/state
```

## Processes

No process was started that outlived a command: every build and every test
ran inside a Herd job on the queue's own runner, submitted with
`herdctl run`, which waits and exits with the job's code. Round one used
`j0000000111`, `j0000000112` and one job per gate through `j0000000121`;
round two, for the apparent-size paragraph, used four more. Nothing was
signalled, nothing was killed, no `pkill` and no kill by name. **PID
662711, the queue's runner, was not touched.**

One background shell waited for `./scripts/bench-preflight.sh` to stop
refusing and exited on its own when it cleared.

`du -sh` of the build tree, **dev profile** (debug, with tests),
`/mnt/ssd_980/herd/build/orch-282-weigh-records`: **14G** after round
one, deleted after that push; round two rebuilt it from cold and it is
deleted again once this push lands. Both figures and both deletions are
reported verbatim in the agent's report.
