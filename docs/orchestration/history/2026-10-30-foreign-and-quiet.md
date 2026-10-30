# foreign-and-quiet — handoff 2026-10-30

- **Item:** the startup-sequence audit of 2026-10-30, findings **F3** and
  **F4** (the two `NONE` rows it ranked above everything it did not get),
  plus the `;`-versus-`&&` gotcha; board rows below
- **Agent:** herd-step
- **Branch / commit:** `foreign-and-quiet` / `396de76` for the three
  changes, plus this handoff as a fourth commit, based on
  `origin/audit-tests` `81132eb`, itself a descendant of `store-guards` →
  `pack-registry`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

Three commits, each landable on its own: 190 lines in `killed_hub.rs`, a
new 275-line test file, and 78 lines of prose. **No production line
changed** — the whole diff is `herd-hub/tests/` and
`docs/orchestration/gotchas/`:

```
 docs/orchestration/gotchas/README.md               |   1 +
 .../a-gate-joined-by-a-semicolon-is-advice.md      |  77 ++++++
 herd-hub/tests/killed_hub.rs                       | 191 +++++++++++--
 herd-hub/tests/quiet_runner_displaced.rs           | 275 +++++++++++++++
 4 files changed, 543 insertions(+), 1 deletion(-)
```

### Commit one — F3, the swept collection of a recreated state root

`86e645d`, `herd-hub/tests/killed_hub.rs` only.

The audit's F3: `settle_against_state_root` has two blocks, and only the
verdict one had a real-binary test. The collection block twenty lines
below it had an in-crate test of the *sentence* it formats, off a
hand-built `Swept` — no hub, no restart, no sequence — and it is the only
**wrong-bytes** failure mode on the audit's whole list: one job's output
files attached to another job's record, with nothing in any log saying so.

Rather than a second forty-line recreation of a state root, the fixture
that already does it is extended, which is what the audit itself proposed:
`a_verdict_spooled_for_a_state_root_that_was_recreated_is_never_believed`
becomes
`a_verdict_and_a_collection_spooled_for_a_recreated_state_root_are_refused`.
One real hub killed and restarted, one real state root `rm -rf`'d and
recreated, one real runner restarted on the same data root, and now **two**
things on that disk instead of one.

**The alignment, which is the fixture.** The planted spool is
`outputs/{job}-1`, and all three of the things the runner's own comment
says must line up for the bytes to land on the wrong record do:

- the **job id** is the one the fresh root hands out again — the test
  already walks the new root to that id by submitting its predecessors,
  and asserts `second == job`;
- the **attempt** is 1, and the new root's job is asserted to have exactly
  one attempt, numbered 1;
- the **runner** is `the-real-runner`, asserted off that attempt's own
  record — the same name that holds the data root the spool is on.

A spool lining up on two of the three would be refused for a reason that
has nothing to do with the state root, and the test would pass while
proving nothing. Two files, one of them a directory down, so the sweep's
walk really walks; the weight in the asserted sentence is computed from
the fixture's own constant rather than typed twice.

**What it asserts, beyond the sentence:**

1. the collection's own refusal line, waited for by *its* text and not by
   the verdict's — both contain `NOT offered`, and reading one as evidence
   about the other is a defect this file has already had two sightings of;
2. `1 collection(s) spooled here cannot belong to this hub's state`;
3. the spool is set aside as `outputs/{job}-1.foreign-state-root`, the
   original path is gone, and **every byte of both files still reads**
   under the new name (D19);
4. the verdict is set aside the same way (the assertion that was already
   there);
5. **never offered**: `it is offered to the hub again` appears nowhere in
   the runner's output, for this job or any other;
6. **nothing of it reached any record of the new root** — the live job's
   record carries no `outputs` at all, its `files/` directory is empty,
   and neither planted file name nor the planted bytes appear anywhere in
   the whole state root, read through `Herd::state_bytes`.

### Commit two — F4, the quiet incumbent displaced on an open link

`3bcb7ca`, new file `herd-hub/tests/quiet_runner_displaced.rs`, 275 lines.

`register_runner`'s third way out — *"runner X has said nothing for D"*,
which kicks the connection the hub is holding and admits the newcomer —
had **no test of any kind**, which the audit measured as `rg "has said
nothing for"` returning zero over every test directory. It fires when the
incumbent has reported at least once and has been quiet for longer than
`health_is_stale_after()` = `lost_after / 2`, **while its socket is still
open**, and `capabilities.rs:769-778` deliberately waits for the hub to
let go of the dead connection first.

Nothing produced that state because killing a runner closes its socket.
The incumbent here is suspended with `SIGSTOP` — `owner_control.rs`'s own
device for a peer that is plainly there and answers nothing — and
`SIGCONT`ed before the test ends. Real hub, two real runners under one
name, `--lost-after 8`.

**How the ordering is the test's and not the scheduler's**, which the
brief named as the whole difficulty. The displacement lives in a window —
after `lost_after / 2`, before the watchdog drops the link at
`lost_after` — and neither bound is guessed:

- *that the incumbent reported* is read off `health_reports_seen` in the
  hub's **published status document**, not inferred from a cadence;
- *that it has been quiet long enough* is read off the same document's
  `last_health_at` — the very `Timestamp` `State::record_health` writes on
  the line below the `Instant` the hub's own `quiet_for` is measured from,
  so the test waits on the hub's own copy of the clause it is about to
  take, re-reading it each turn so a frame still in flight at the stop
  moves the reading instead of invalidating it;
- `lost_after` itself is read back out of the document (`lost_after_ms`)
  and asserted equal to what the command line asked for, so the halving
  is not a figure typed twice;
- *that the watchdog had not already acted* is asserted afterwards from
  the hub's own output: `register_runner` can print *"has said nothing
  for"* only while it still holds the incumbent, and the watchdog's
  *"nothing has come back on it for"* must be **absent**. A machine too
  slow to make the window therefore fails rather than passing for the
  wrong reason — demonstrated, not claimed (Finding B).

Four more assertions beside those: the newcomer was **not** told to stand
by (`register_runner`'s third way out), `runner the-real-runner has
joined` appears exactly **twice** under the one name, a job submitted
after the displacement runs and its output is the twin's, and the name is
still held by a runner the hub is hearing from.

### Commit three — the gotcha

`396de76`, new file
`docs/orchestration/gotchas/a-gate-joined-by-a-semicolon-is-advice.md`
plus one row in that directory's `README.md` index.

## Not done, and why

- **Nothing of F5, F6, F7 or the rest of the audit's `NONE` column.** Out
  of this brief, and carried in the board row below.
- **No script change for part three**, as the brief required: making the
  preflight refuse to be ignored is drafted as a board row, not built.
- `docs/orchestration/board.md` and `docs/decisions.md` are untouched, per
  the brief and D226: every row and entry is drafted here in full.
- **No `D-NEXT` decision entry is drafted.** The brief asked for none, and
  neither test decides anything: they assert behaviour that is already
  decided (D19, D34, the state root in `Welcome`) and the gotcha is a
  working rule, not a product decision.

## Risks and findings

### Finding A — what the code refutes in the audit's F3

The audit's F3 says the hazard is *"one job's output files attached to
another job's record"*, and the runner's comment says *"or, where the
attempt and the runner happen to line up, written onto that other job's
record"*. **The alignment the comment names is necessary but not
sufficient**, and the brief's phrase *"so that the attempt and the runner
do line up on the new root — that is the dangerous alignment"* is
therefore the right fixture for the *refusal* but would not, on its own,
produce the landing.

Reading the hub's side: a collection is written onto a record only where
the hub is **owed** it, and `collection_owed` (`store.rs:3017`) answers
`None` on its first two clauses —
`if !record.outputs.streamed || record.outputs.files.is_empty()` — before
it ever looks at wholeness or at the attempt. On a **freshly recreated**
root the new `j0000000001` has streamed nothing, so the hub is owed
nothing for it, says nothing about it at join, and answers an offer of it
from that same record. So the wrong-bytes landing needs a **fourth**
coincidence on top of the three: the new root must itself be owed a
collection for that job and that attempt — the new job streamed output and
the acknowledgement was lost. Had the runner not refused, the planted
spool would have met silence and been condemned as `.unclaimed`: a
different defect, and a cheaper one.

What this changes: nothing about the test, which asserts the refusal where
the refusal is made, plus the consequence (nothing in the new root) rather
than only the sentence. What it changes is the **severity** language: F3's
*"the only wrong-bytes failure mode on the list"* is right about what the
code would do if the guard were removed **and** the fourth coincidence
held, and the guard is one `if mine == Some(hub_root)` wide. The
consequence assertion in the test is therefore a *consequence*, not a
detector that was proven to fire; the detector that was proven to fire is
the pair of assertions on the runner's own sentence and rename (Finding
B).

### Finding B — both tests were demonstrated red first

A green test proves nothing until its own failure has been seen.

- **Test one.** With the two planted files not written (one hunk of the
  fixture removed, never committed), the test went **red** at exactly the
  new wait: `timed out after 30s waiting until the runner says the foreign
  collection is not offered`. So the whole new block is driven by the
  planted spool and not by anything ambient in the fixture.
- **Test two.** With `PAST_THE_THRESHOLD` raised from 500 ms to 4500 ms —
  which puts the dial past `lost_after` instead of past `lost_after / 2` —
  the test went **red**, and the hub's own output in the failure said why:
  `dropping the connection to runner the-real-runner: nothing has come
  back on it for 8.318273887s`. That one run is worth three claims: the
  window the test aims at is real; the watchdog line the test refuses to
  see is a line the hub really does print in the other case; and a run
  that loses the race **fails** rather than passing for the wrong reason.
  It is recorded in the test file's own header, with the figure.

### Finding C — F4 is right about `capabilities.rs`

F4 says `capabilities.rs:769-778` *"deliberately avoids this branch"*. It
does, and its comment is a better argument than the audit's: killing the
runner and dialling straight after was measured as *"the hub had **not**
let go at that instant in eight runs of eight, and took 220 to 640 ms to
notice"*. That is the same measurement my test needs in the other
direction, and it is why the incumbent here is suspended rather than
killed: there is no way to produce *reported-then-silent, still
connected* from a process that has exited.

### Finding D — the audit's thirty seconds checked out, and it has a twin

I went after F4's other figure expecting to refute it — *"a name that
cannot be replaced for thirty seconds"* — because `Policy` has a second,
much shorter retry: `stand_by_retry()`, `(lost_after / 20).clamp(200 ms,
5 s)`, which is **3 s** at the deployed 60. **The audit is right and I was
wrong.** The two are different stand-bys:

- a **twin** refused because the name is held and the incumbent is being
  heard from gets `retry_in: self.policy.health_is_stale_after()` —
  `lost_after / 2`, **30 s** deployed (`server.rs:1792`);
- `stand_by_retry()` is used in exactly one other place, for a runner
  whose **data root belongs to another hub's state root**
  (`server.rs:3182`), which is a different refusal with a different
  cadence.

So F4's thirty seconds is the figure a twin really waits, and the 3 s next
to it in the same struct is about something else. Recorded because the
number is one line from a number that looks like it and is not.

### Finding E — the README index row is one path the brief did not name

The brief gave me the gotchas *file*. I also added one row to
`docs/orchestration/gotchas/README.md`, whose table is the index every
other gotcha is listed in and is how an agent meets the file at all. It
is in the same directory, it is not on the brief's off-limits list, and
`check-docs.sh` is green on it — but it is a path the brief did not name,
so it is reported here rather than assumed.

### Finding F — what test two does not assert, said rather than implied

The audit's consequence for F4 is *"both processes briefly believing they
hold one attempt"*. Test two asserts the **displacement** and that the
newcomer serves; it does **not** produce two processes holding one
attempt, because no job is in flight on the suspended incumbent when the
twin dials. Making it so is a bigger fixture — a job placed on the
incumbent, the incumbent suspended mid-job, the twin admitted, and then
the two sides' claims about that attempt compared — and it belongs with
F5, the watchdog's `what_the_runner_says` path (D169), rather than here.
It is in this handoff and not in the test file because the five runs and
the suite are evidence about the committed text, and the text was not
changed after them.

## Gates

All four Rust gates plus `check-docs.sh`, on the tree of `396de76` (the
three commits) — run **before** this file was written, which is why their
output can be quoted here.

`nix develop --command cargo build --workspace --all-targets`:

```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 08s
```

`nix develop --command cargo clippy --workspace --all-targets -- -D warnings`:

```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 15.64s
```

`nix develop --command cargo fmt --all --check`: clean. It was **not**
clean on the first attempt — rustfmt wanted four call sites in the two
test files reflowed — so `cargo fmt --all` was run, the result rechecked
clean, and the three commits were rebuilt on the formatted tree rather
than a fourth "format" commit added. Every figure below is from after
that.

`nix develop --command ./scripts/test.sh`
(`…/scratchpad/fq-suite.log`):

```
test suite: PASSED
```

Both new tests inside that run:

```
test a_verdict_and_a_collection_spooled_for_a_recreated_state_root_are_refused ... ok
test a_runner_that_went_quiet_on_an_open_link_is_displaced_by_its_twin ... ok
```

**Skips: 12, and the same 12 names `scripts/expected-skips/local.txt`
carries** — every one a cgroup-v2 delegation or hard-IO-ceiling assertion
this host cannot make. **None comes from either new test**, and
`scripts/` is untouched. Printed as skipped and never counted as passed
(D27):

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

`./scripts/check-docs.sh`:

```
docs ok
```

### The figure asked for before anything was written

`nix develop --command cargo test -p herd-hub --lib`, on the unmodified
base `81132eb`, before a line of this branch existed — a real test target
that compiled and ran, not `cargo check --lib`:

```
test result: ok. 258 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 8.87s
```

Unchanged by this branch: neither commit touches `herd-hub/src`, and both
new tests are integration tests, so `--lib` still reports 258.

### Test two, five runs, verbatim

`cargo test -p herd-hub --test quiet_runner_displaced` under
`nix develop --command`, five separate invocations **on the formatted
tree that is committed**, each preceded by its own preflight:

```
--- run 1 2026-10-30T04:03:35+01:00
test a_runner_that_went_quiet_on_an_open_link_is_displaced_by_its_twin ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 18.21s
--- run 2 2026-10-30T04:03:55+01:00
test a_runner_that_went_quiet_on_an_open_link_is_displaced_by_its_twin ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 21.66s
--- run 3 2026-10-30T04:04:29+01:00
test a_runner_that_went_quiet_on_an_open_link_is_displaced_by_its_twin ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 27.91s
--- run 4 2026-10-30T04:05:19+01:00
test a_runner_that_went_quiet_on_an_open_link_is_displaced_by_its_twin ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 15.89s
--- run 5 2026-10-30T04:05:45+01:00
test a_runner_that_went_quiet_on_an_open_link_is_displaced_by_its_twin ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 16.54s
```

Five of five, 15.89 s to 27.91 s, on a host whose load was between 2 and
8 across them — the spread is the host, not the window: the wait that
decides the window is measured on the hub's own record and not on a
sleep. A sixth run inside `./scripts/test.sh` makes six, and `gate.sh`'s
own suite makes seven. One earlier set of five, on the text before
rustfmt reflowed four call sites, was also 5/5 (14.52-15.66 s).

### Test one, its own run and its demonstrated red

`cargo test -p herd-hub --test killed_hub a_verdict_and_a_collection`:

```
test a_verdict_and_a_collection_spooled_for_a_recreated_state_root_are_refused ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 20.12s
```

With the two planted files not written and nothing else changed:

```
timed out after 30s waiting until the runner says the foreign collection is not offered.
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 42.93s
```

### Test two's demonstrated red

With `PAST_THE_THRESHOLD` at 4500 ms instead of 500 ms, which puts the
dial past `lost_after` instead of past `lost_after / 2`:

```
timed out after 30s waiting until the hub's record of the incumbent's last report is older than its threshold.
herd-hub: runner the-real-runner has joined
herd-hub: dropping the connection to runner the-real-runner: nothing has come back on it for 8.318273887s
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 39.56s
```

The watchdog had freed the name, the runner was gone from the status
document, and the wait ran out. Both margins were restored and both
demonstrations are **not** in any commit.

### The gotcha's own verify step, run both ways

```
--- with &&:
bench-preflight: only 363 GiB free on /mnt/ssd_980, need 999999. Free space or point CARGO_TARGET_DIR at a roomier volume; on this host, delete the target/ of finished worktrees: see docs/orchestration/gotchas/build-weight-off-root.md
bench-preflight: REFUSED, do not start the heavy phase.
--- with ; :
bench-preflight: only 363 GiB free on /mnt/ssd_980, need 999999. Free space or point CARGO_TARGET_DIR at a roomier volume; on this host, delete the target/ of finished worktrees: see docs/orchestration/gotchas/build-weight-off-root.md
bench-preflight: REFUSED, do not start the heavy phase.
THE PHASE WOULD HAVE STARTED
```

`HERD_PREFLIGHT_MIN_FREE_GIB` only, never
`HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB`, never exported into any gate's
environment (ORCH-209).

### The last run, named and not quoted

`./scripts/gate.sh`, bare — **never inside `nix develop`**, which nests
`TMPDIR` and panics `confinement.rs` on the 108-byte `SUN_LEN`. Run on
the committed tree including this file, with its output at
`…/scratchpad/fq-gate.log`. The verdict is in my report to the
orchestrator, not in the tree it gated. The branch is pushed only on
green.

### The host, and the margin I started on

`./scripts/bench-preflight.sh` before every heavy phase, **joined to it
with `&&` and never `;`** — which is this task's own part three, and the
join did its work: at 04:00:51 the preflight refused on a load of 11.25
against a ceiling of 9.00 and **clippy and the suite did not run**, which
is visible in the log as `clippy exit=1` with no cargo output under it.

The margin rule is another agent's measurement, taken rather than
invented: **two readings 20 s apart, both clearing, both at least
4608 MiB above the script's own 8192 MiB floor.** What I started on, per
phase:

```
03:41:07 load=0.72 room=16475 verdict=ok   (baseline --lib)
03:41:35 load=0.82 room=16464 verdict=ok   (the second of the pair)
03:59:41 load=2.78 room=15799 verdict=ok   (build)
04:02:38 load=2.84 room=16674 verdict=ok   (clippy, first of the pair)
04:02:58 load=2.36 room=16713 verdict=ok   (clippy)
04:03:15 load=2.64 room=16698 verdict=ok   (five runs, first of the pair)
04:03:35 load=2.04 room=16702 verdict=ok   (five runs)
04:06:53 load=8.78 room=13282 verdict=ok   (suite, first of the pair)
04:07:13 load=8.20 room=13238 verdict=ok   (suite)
```

**A plateau, not a peak**, on every pair: 16475/16464, 16674/16713,
16698/16702 and 13282/13238 — the two readings of each pair agree to
within 50 MiB, and the thinnest margin any phase started on was
**5046 MiB above the floor** (the suite). **This figure is not a
threshold and must not be read as one**: the rule is the margin on two
readings, and another agent withdrew exactly this kind of figure tonight
after a clear 1879 MiB above the floor met both of the script's own
clauses. One refusal was waited out (04:06:33, load 10.14) rather than
worked around; `HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB` was never set,
nothing under `/sys/fs/cgroup` was written, and no cache was dropped.

## Drafted for `herd-merger`

No `docs/decisions.md` entry. Two `board.md` rows, in full.

### Board row one — F3 and F4 are closed; F5 to F7 are what is left

This **replaces** `history/2026-10-30-audit-tests.md`'s *Board row three*
(drafted there as `ORCH-NEXT+2`), which listed F3 to F6 as owed. If that
row has already been allocated a number, this text is its new *Item* and
*Next action*; if it has not, this row supersedes it.

```markdown
| ORCH-NEXT | **The startup audit's F3 and F4 are tested; F5, F6 and F7 are not.** `foreign-and-quiet` closed the two the audit ranked highest among its `NONE` rows: **F3/R2b**, a swept collection on a recreated state root — the only wrong-bytes failure mode on the list — now asserted through the real binaries in `killed_hub.rs`, with the aligned spool and with the new state root scanned for any trace of it; and **F4/H1b**, the quiet-incumbent displacement, which had no test of any kind, now `herd-hub/tests/quiet_runner_displaced.rs` with a `SIGSTOP`ped incumbent on an open link. What is left has no test in any harness: **F5/H2a-H2b**, the watchdog's two checks, best covered today by a test that cuts a real TCP relay against an *in-process* hub and whose deployed-threshold twin is `#[ignore]`d; **F6/R3**, the runner's 60 s `HUB_SILENCE_LIMIT`, asserted by nothing anywhere — `rg "so this link is dead"` returns zero; **F7/R4**, `MOST_ENTRIES_SWEPT` = 50 000 against a consumer scale of ~69 600 entries, where `names.sort()` makes the starved tail deterministic | the startup audit of 2026-10-30, F3-F7; closed in part by `foreign-and-quiet` | herd-step | owed | **F6 next, and it is the cheapest of the three**: one real runner against a hub that stops sending frames, which `killed_hub.rs` already knows how to make. F5 after it, because the in-process test does produce the absence the production way and the gap is the harness. F7 is a *figure*, not a test: measure the entries per spool a consumer pack produces before the pack half multiplies them, then decide whether the bound moves | — | `…/scratchpad/2026-10-30-startup-sequence-audit.md` F3-F7, rows R2b, R3, R4, H1b, H2a, H2b; `herd-hub/tests/killed_hub.rs`; `herd-hub/tests/quiet_runner_displaced.rs`; `history/2026-10-30-foreign-and-quiet.md` |
```

### Board row two — make the preflight hard to ignore, not just loud

The brief asked for this as a row rather than as code, and I agree with
that order: the prose rule is cheap and the mechanism needs a decision
first.

```markdown
| ORCH-NEXT+1 | **`bench-preflight.sh` cannot tell whether anybody acted on its exit status, and three agents in one night did not.** All three joined the preflight to the heavy phase with `;` instead of `&&`, so the refusal printed and the phase started anyway; the lesson is written down (`gotchas/a-gate-joined-by-a-semicolon-is-advice.md`) and prose is all that has been done. Two mechanisms were considered and neither is built: **(a)** a `--run` mode that executes the guarded phase itself — `./scripts/bench-preflight.sh --run -- nix develop --command ./scripts/test.sh` — so there is no join to get wrong and the figures and the phase are one process; **(b)** a refusal that drops a short-lived marker in the agent's own scratchpad which `test.sh` and `gate.sh` refuse to start against | three execution agents, 2026-10-30, each self-disclosed; `history/2026-10-30-foreign-and-quiet.md` | herd-planner | open | Decide (a) or (b) before writing either. Recommend **(a)**: one script, no state, and it keeps the four figures and the phase in one process; (b) catches a phase started from another shell but adds state that goes stale and a second way to be wrong. Note before starting: `check-orchestration.sh` already drives `bench-preflight.sh` under a controlled environment to assert what a refusal *says* (ORCH-209), so its output is pinned by a test and a new mode moves with it | — | `docs/orchestration/gotchas/a-gate-joined-by-a-semicolon-is-advice.md`; `scripts/bench-preflight.sh`; ORCH-209 |
```

## Decisions needed

None. Both tests assert behaviour that is already decided, the gotcha is a
working rule rather than a product fact, and the one thing I would change
in the audit's own text — F3's severity sentence — is written down as a
finding rather than put to anybody.

## Resume point

The branch is pushed, test-only plus one gotcha, over `origin/audit-tests`,
and lands after the batch of ten. For whoever continues the audit's work,
the cheapest remaining row:

```sh
# F6: the runner's own silence limit, asserted by nothing anywhere
git worktree add <path> -b runner-silence-limit origin/main
# then: a real runner against a hub that stops sending frames, and assert
# the runner's "so this link is dead" and its redial. killed_hub.rs already
# kills a hub; what is new is a hub that is up and says nothing.
```

## Processes

- Every process I started was a `nix develop --command cargo …` shell of
  my own and the hubs, runners and `herdctl` the tests spawn through
  `real_herd`, which kills and reaps them on drop. Nothing was killed by
  name: no `pkill`, no `killall`.
- **Test two sends signals, and only to pids it started**, through
  `real_herd::Process`, which reads the pid out of its own `Child`:
  `SIGSTOP`, then `SIGCONT`, then `SIGTERM`. **Nothing is left stopped** —
  checked after the runs with `ps -eo pid,stat,cmd | awk '$2 ~ /T/'`,
  which listed nothing at all.
- The first consumer's `herd-hub` (3006543) and `herd-runner` (3021002)
  were untouched and are still up; the ceiling of two heavy agents was
  respected, with one compile at a time.
- `CARGO_TARGET_DIR` was `/mnt/ssd_980/herd/build/foreign-and-quiet`,
  nothing on the root filesystem, and it is removed once the branch is
  pushed.
