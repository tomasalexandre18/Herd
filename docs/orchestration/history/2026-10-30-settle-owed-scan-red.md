# settle-owed-scan-red — handoff 2026-10-30

- **Item:** the red gate of batch 26 (ORCH-197's test, D225's fixture)
- **Agent:** herd-step
- **Branch / commit:** `settle-owed-scan-red` / the commit this file is
  in, based on `origin/batch-26-integration`
  `22f9846a5142ba817b43546471e6501322b1ed36`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing and fills the record of `templates/landing.md`

## Done

**The red is settled, and it is a fixture defect. The hub is innocent and
there is no product fallback.** `herd-hub/src` is untouched by this
branch.

### What failed, and why

`herd-hub/tests/a_failed_listing/mod.rs` breaks a listing by leaving the
hub's `jobs` directory at mode `0o111` — search, no read — so
`fs::read_dir` fails with `EACCES` while `open("jobs/<id>/job.json")`
still succeeds. It verified the mode bound and then **assumed it would
keep binding**. It does not.

The suite runs inside a Herd job. The runner carrying that job holds it
to its disk budget by walking the job's whole base every
`POLL_INTERVAL` — 200 ms, `herd-runner/src/confine.rs:106` — from
`herd-runner/src/job.rs:1555`, `workspace.measure()`, which is
`measure_tree(&self.base, &self.root)` at `herd-runner/src/root.rs:443`.
`tmp` is **inside** that base (`root.rs:317`, `let tmp =
base.join("tmp")`), and every temporary state root in this suite is a
`tempfile::tempdir()` under `$TMPDIR`. Any directory that walk cannot
list has the runner's own access handed back to it:
`read_dir_restoring` (`root.rs:538`) calls `restore_access`
(`root.rs:664`), which is `set_permissions(mode | 0o700)`.

So `0o111` becomes `0o711`, `read_dir` works again, and a hub that scans
in that window scans **successfully** and correctly names what it is
owed. That is what the gate saw. The hub log's *"the-first-machine still
owes this hub files of j0000000001; asking it to resume"* is printed only
inside the `Ok` branch of `ask_for_the_files_still_owed`
(`herd-hub/src/server.rs:6730`), reached only when
`JobDirStore::collections_owed` (`store.rs:3190`) succeeds — which is
right for the permissions the hub really had, and no evidence at all
about ORCH-197.

The runner is doing exactly what invariant 2 and D166 require: a job must
not `chmod` its way out of a figure the runner enforces.

### Measured, not argued

**The cause, deterministic, 1 of 1 in Herd job `j0000000147`** — a
directory moded `0o111` under that job's `$TMPDIR`:

```
dir=/mnt/ssd_980/herd/queue/runner-data/jobs/j0000000147/tmp/tmp.uwHGGR1R85
t=0.00s mode=2111 read_dir=EACCES
t=0.10s mode=2111 read_dir=EACCES record=opens
t=0.20s mode=2711 read_dir=ok     record=opens
t=0.30s mode=2711 read_dir=ok     record=opens
```

`0o711` where the fixture set `0o111` — and never the guard's own
`0o755` — is the fingerprint: it is `mode | 0o700`.

**The failure, deterministic, 10 of 10**, on the **unmodified gated
binary** (`owed_scan_fails-045c1bc641b099f5`, byte-identical hash to the
gate's), with one thing varied: a faithful stand-in for
`restore_access` over the suite's `$TMPDIR`, one restore per path.

```
D-pristine-under-restorer: 10 red of 10
restores performed: 10
restored .../.tmpByxhFT/jobs from 0111 to 0711
one failed listing is silent to every runner: [JobFilesHeld { job_id:
JobId("j0000000001"), attempt: 1, held: [] }, HealthRecorded]
```

That message is byte-identical to the gate's. **With the restorer off,
the same binary is 0 red of 10 filtered and 0 red of 10 unfiltered
locally, and 0 red of 20 inside a Herd job** — which is why the previous
agent's five-of-five control was green and why this hid.

**Not concurrency: duration.** The batch is not implicated by any route.
The window a run exposes is the few milliseconds between the `chmod` and
the frames being read; the walk comes every 200 ms. The gate job ran
390 772 ms — about 1 950 walks — against a control of roughly 0.3 s. The
collision is a per-measurement lottery of a few percent, not a
concurrency effect, and `owed_scan_fails.rs` shares no code with the
batch's changes.

### The fix: the premise is guarded, no assertion is loosened

`herd-hub/tests/a_failed_listing/mod.rs`

- `over` now requires the verification failure to be
  `ErrorKind::PermissionDenied`. A listing that failed for a reason
  nobody arranged — a descriptor limit, a tree that moved — used to be
  read as the fixture binding; it now panics naming the error.
- `still_binds()` asks whether the listing is **still** failing, with
  the whole mechanism written down beside it.
- `hold_again()` re-breaks it: a retry of the **fixture**, never of an
  assertion.
- `ATTEMPTS = 8` and `WOULD_NOT_HOLD` bound it, so a host that restores
  access faster than the fixture can set it is **red with a sentence**.

`herd-hub/tests/owed_scan_fails.rs` — both tests that assert the silence
at join now take the measurement, **then** check the fixture, and take it
again from nothing if the host undid it. Nothing is slept on, nothing is
`#[ignore]`d, no expectation is widened, and the two assertions are the
same text they were. The check is conservative in the safe direction: it
can only void a measurement, never pass a broken one, because the only
code that sets `0o111` is the fixture.

`herd-hub/tests/owed_scan_fails_against_the_real_runner.rs` — comment and
failure message only. Its fixture guard already exists and is the right
one: the hub's own *"cannot tell which collections are outstanding"* line
is the only honest witness of **when** the scan ran. A mode read at the
end of that test would be read after the window, so asserting on it
would have turned legitimate greens red. It cannot retake its
measurement — restarting a real binary mid-test would mix two lives' log
lines into the counts it reads — so it stays red-and-explained, and the
drafted row carries it.

### The fix proved both ways

| Condition | Binary | Result |
|---|---|---|
| restorer on, one restore per path | gated, unmodified | **10 red of 10** |
| restorer on, one restore per path | fixed, one test | **0 red of 10** |
| restorer on, one restore per path | fixed, whole binary | **0 red of 10** |
| restorer **continuous** | fixed, one test | **red**, `WOULD_NOT_HOLD` |
| restorer off, in a Herd job | gated, whole binary | 0 red of 20 |

The restore still fires on every run of the green rows — 10 and 30
restores recorded — so the green is the retake working, not the
condition going away. The continuous row is the proof the fix cannot
become a hollow green: eight attempts, eight restores, then

```
thread '...' panicked at herd-hub/tests/owed_scan_fails.rs:501:17:
this host handed the jobs directory its read permission back inside
every attempt, so the hub never scanned a broken listing and nothing
here is evidence about ORCH-197. [...] 0o711 where the fixture set
0o111 is the fingerprint
```

And it is run **alone and in the full suite**: alone above, in the suite
under *Gates*. Alone-green is exactly what hid this.

## Not done, and why

- **The class is not fixed, only named.** Every suite fixture that
  depends on a directory staying unreadable is exposed the same way. The
  members are listed under *Risks and findings* and carry a drafted row.
  Fixing them is not this task and three of them are latently red
  tonight.
- **`owed_scan_fails_against_the_real_runner.rs` still cannot retake its
  measurement.** Reasoned above; it is in the drafted row.
- **`batch-26-integration` was not re-gated and nothing was landed.**
  `herd-merger`'s, off this branch.
- **No product change anywhere.** `herd-hub/src`, `herd-runner/src` and
  `herdctl/src` are untouched.

## Gates

This is a test change, so all four Rust gates plus `check-docs.sh` were
run, through Herd, on the **committed** tree —
`7e5abd5345da8c4fd2bb4d1cca908317994a7f52`, `git status` clean — in job
`j0000000155`, before this file was added. All five green:

```
tree sha: 7e5abd5345da8c4fd2bb4d1cca908317994a7f52
GATE1 exit=0   cargo fmt --all --check
GATE2 exit=0   cargo build --workspace
GATE3 exit=0   cargo clippy --workspace --all-targets -- -D warnings
GATE4 exit=0   ./scripts/test.sh
   12 assertion(s) COULD NOT RUN on this host and were skipped.
   ignored census: COMPLETE (38 ignored, 0 without a reason)
   test suite: PASSED
GATE5 exit=0   ./scripts/check-docs.sh  ->  docs ok
RATE: 0 red of 200 whole-binary runs inside this Herd job
```

**Skips, as a list and not a count (D187).** The 12 `HERD-SKIPPED` names
and the 38 `HERD-IGNORED` names were diffed **by name** against the
gate's own anchored `allowed-skips-22f9846a5142.txt` and
`allowed-ignored-22f9846a5142.txt`: both identical, nothing added,
nothing lost. All 12 are the cgroup-delegation family this host cannot
carry, and **none of them is in `owed_scan_fails`** — so the green was
not bought with a skip. Full output in
`settle-red-evidence/gates-final.log`.

**The 200-run line is about the fixed binary**, run in the environment
that produced the red: whole binary, three fixtures a run, 600 fixture
exposures, no red. It bounds the fixed test's rate inside a Herd job at
under roughly 2 per 100 runs at 95 per cent confidence. It says nothing
about the unfixed one, which was measured separately and deterministically
under the stand-in.

The **last run** is `./scripts/check-docs.sh` on the tree with this file
committed. Its output goes to
`settle-red-evidence/check-docs-last.txt` and into the agent's report,
not into the tree it gates.

## Risks and findings

### 1. This is the measured cost of today's own directive

*"Everything through Herd, no more local execution"* is **D258**, dated
today, and the orchestrator wrote the rule. Its consequence, measured
tonight: **a gate run through Herd is not equivalent to a gate run
locally, for a whole class of assertions.** Running the suite inside a
Herd job puts its fixtures under a workspace that is walked and
access-restored every 200 ms, so a fixture that sets a mode and reasons
from it is sound locally and unsound through Herd. Not blame —
provenance, so the next reader sees it beside the rule instead of
discovering it in a red gate at 20:00. The drafted sentence for
`docs/orchestration/workflow.md` is below.

### 2. The class, with its members

Only **directories the walk cannot read** are restored: `restore_access`
returns early on anything that is not a directory, and it is reached only
when `collect_dir` failed. So a file moded `0o000` is safe, and a
directory moded `0o500` is safe — the walk reads it fine. Unreadable
directories under `$TMPDIR` are the class.

**Loud — the premise dissolves and the test fails.** Safe, but latently
red:

- `herd-hub/tests/owed_scan_fails.rs`, both assertions of the silence at
  join — **this is the one that fired**; fixed on this branch.
- `herd-hub/tests/owed_scan_fails_against_the_real_runner.rs`,
  assertions 1 and 2. **The widest window in the suite** — the whole of
  a real runner's start-up — and it cannot retake.
- `herd-hub/src/server.rs:11455`, the same `0o111` fixture duplicated in
  that file's unit tests.
- `herd-runner/src/sweep.rs:708` and `:786`
  (`a_spool_that_will_not_read_through_is_not_offered_and_says_which_path`,
  `several_unreadable_paths_name_the_first_and_count_the_rest`) — a
  restored directory reads through, the spool is offered, the assertion
  fails.
- `herd-runner/src/root.rs:1100`, the test named
  `a_path_the_runner_cannot_take_its_access_back_on_is_refused`
  `_and_not_capped` — it puts the unreadable directory **above** the
  inner data root so the product's own restoration cannot reach it, but
  the **outer** runner's `up_to` is the outer data root and reaches it,
  so `measure()` returns `Ok` where the test demands `Err(refused)`.

**Silent — the premise dissolves and the test still passes.** This is
answer 3 below and the dangerous direction.

**Not exposed, checked and ruled out:** `herd-hub/tests/durability.rs`
`:1107` is `0o000` on `job.json`, a **file** — `restore_access` skips
non-directories; `herd-hub/tests/killed_hub.rs` `:192`, `:490` and
`herd-hub/src/store.rs:4918` are `0o500` on directories, which list
fine; `herd-runner/src/spool.rs:463` likewise; `capabilities.rs:656`
grants `0o755`; `herd-runner/src/privilege.rs` and
`herdctl/tests/fixture/mod.rs` act on files.

### 3. Yes — there are fixtures that would go GREEN on a dissolved premise

This is the shape that makes a green gate a lie, and it exists. **Three
tests, by name**, all of them of the form *"it works despite the mode"*:

- `herd-runner/tests/confinement.rs`
  `::a_job_cannot_hide_what_it_wrote_from_its_disk_budget`
  — **the worst of them.** The job runs `chmod 300 h; sleep 6`, so the
  premise is exposed for **six seconds** against a 200 ms walk: the
  outer runner restores `h` to `0o700` with near-certainty long before
  the inner runner measures. The test then asserts
  `StoppedByCap { Disk }`, which holds whether or not the inner runner
  had to take any access back. The defect it is named for — *"a job that
  blinds the runner must still be held to its budget"* — is the one
  thing it no longer exercises when run through Herd.
- `herd-runner/src/root.rs`
  `::a_directory_a_job_made_unreadable_is_still_measured`
  (`0o300`, then `0o000` for the reclaim half) and
  `herd-runner/src/root.rs::a_workspace_a_job_locked_shut_is_still_reclaimed`
  (`0o000` on `d`) — both assert success despite the mode. Windows are
  sub-millisecond, so the hole is roughly `window / 200 ms`: small, and
  in the wrong direction.
- `herd-runner/src/outputs.rs::a_directory_the_job_closed_is_still_collected`
  (`0o000` on `{out}/files`) — same shape, same small window.
- `herdctl/tests/exit_code.rs:756` carries the same `chmod 300 h` case
  with a short window.

**What that means, stated with the direction of its error.** A
regression that deleted `restore_access` or `read_dir_restoring` would
still be caught by these tests in the great majority of runs — their
windows are milliseconds — **except**
`a_job_cannot_hide_what_it_wrote_from_its_disk_budget`, whose six-second
window makes it effectively hollow through Herd: it would stay green
with the product's restoration removed. That one is a test that proves
nothing it claims, tonight, and it outranks everything else here.

**How this was looked for, so nobody re-derives it.** Every
`set_permissions` in the repo was enumerated (41 sites, `grep -rn
set_permissions --include=*.rs`) plus every `chmod` in tests and scripts;
each was classified by whether its target is a directory the walk cannot
read (the only thing `restore_access` touches) and then by what its test
asserts when the mode comes back — fails loudly, or passes anyway. The
list above is the whole of it. **It is a code reading, not a
measurement**: the mechanism is measured (job `j0000000147`), the window
of each test is read from its source, and no run was made with
`restore_access` removed, which is what a direct measurement of the hole
would need.

### 4. Batch 26's committed handoff carries a landing that did not happen

`docs/orchestration/history/2026-10-30-merge-batch-26.md` on
`batch-26-integration` carries the template line *"**Batch:** 26, landed
2026-10-30 by herd-merger"* and **no landing happened**. Its author
correctly did not amend it — a handoff is committed at step 6 and never
touched again — but if that integration lands on another date the line is
false. The re-gating merger should see this and decide; it is not mine to
fix.

### 5. What a sceptic should still check

The reproduction uses a stand-in for `restore_access` rather than Herd's
own walk, because the real walk's cadence cannot be raised. The stand-in
performs the identical syscall (`chmod mode | 0o700` on a directory it
cannot list), the real mechanism is measured separately in job
`j0000000147`, and the two meet on the same fingerprint, `0o711`. What is
**not** proved is that the gate's single red was that mechanism rather
than a second unknown one with the same signature; the gate's job
workspace is gone, so its mode cannot be read back.

## Drafted for `herd-merger`

`D267` and `ORCH-286` to `ORCH-290` are already allocated on
`batch-26-integration`, so the next free numbers are above those. The
numbers below are placeholders; the merger allocates (D148, invariant 8).

### `docs/decisions.md` — one new entry

```markdown
## D-NEXT — a suite fixture that sets a mode must guard its premise
(2026-10-30)

**A mode this suite sets is not a mode this suite keeps**, since D258
made every gate a Herd job. The runner carrying a job holds it to its
disk budget by walking the job's whole base every 200 ms
(`herd-runner/src/confine.rs:106`, from `job.rs:1555`;
`root.rs:443` `measure_tree(&self.base, &self.root)`), and `tmp` is
inside that base (`root.rs:317`). A directory that walk cannot list has
the runner's own access handed back to it — `read_dir_restoring`
(`root.rs:538`) calls `restore_access` (`root.rs:664`), which is
`set_permissions(mode | 0o700)`. Every temporary state root in the
suite lives under `$TMPDIR`, so a fixture directory left at `0o111`
becomes `0o711` within 200 ms and reads again.

That is the runner obeying invariant 2 and D166: a job must not `chmod`
its way out of a figure the runner enforces. The product is right and
the fixtures are wrong, so **the rule is on the fixtures**: a test whose
premise is a mode checks that premise **at the moment it judges a
measurement**, and a measurement taken after the premise was undone is
**void** — taken again from nothing, a bounded number of times, and
then red with a sentence. Never skipped: a skip would move the suite's
`HERD-SKIPPED` list, which is compared and not counted (D27, D187).
Never slept on, never `#[ignore]`d, and no assertion is widened.

Measured 2026-10-30 in job `j0000000147`: `mode=2111 read_dir=EACCES`
at t=0.10 s, `mode=2711 read_dir=ok` at t=0.20 s. **`0o711` where a
fixture set `0o111` is the fingerprint** — never the guard's own
`0o755`.

**The consequence of D258 this names.** A gate run through Herd is not
equivalent to a gate run locally for this class of assertion: the same
tree is sound locally and unsound through Herd. Batch 26's gate went red
on it after eighteen green attempts — `owed_scan_fails.rs`'s
`one_failed_listing_silences_every_runner_and_each_is_still_answered`,
whose fixture had been un-chmodded under it — and the hub was innocent.
The window a run exposes is a few milliseconds against a walk every
200 ms, which is why it is rare, why it is a function of job **duration**
and not of concurrency, and why a filtered control of 0.3 s was five of
five green while a 390 s suite was red.

Done for the hub's two tests of it in
`herd-hub/tests/a_failed_listing/mod.rs` — `still_binds`, `hold_again`,
`ATTEMPTS`, `WOULD_NOT_HOLD`, and `over` now requiring the verification
failure to be `PermissionDenied` so that a listing which failed for an
unarranged reason is never read as the fixture. Proved both ways against
a faithful stand-in for `restore_access`: the unmodified gated binary 10
red of 10, the fixed one 0 red of 10 alone and 0 red of 10 as a whole
binary with the restore still firing every run, and red with its own
sentence when the stand-in restores continuously.
```

### `docs/orchestration/board.md` — two rows for *Owed*

```markdown
| ORCH-NEXT | **The suite's mode fixtures are unsound through Herd, and three of them are latently red.** D-NEXT names the mechanism; this is the inventory. Fixed: `herd-hub/tests/owed_scan_fails.rs` (both silence-at-join assertions). Latently red, same cause, not fixed: `herd-hub/tests/owed_scan_fails_against_the_real_runner.rs` assertions 1 and 2 — the widest window in the suite, a real runner's whole start-up, and it cannot retake its measurement because restarting a binary mid-test would mix two lives' log lines into the counts it reads; `herd-hub/src/server.rs:11455`, the same `0o111` fixture duplicated in that file's unit tests; `herd-runner/src/sweep.rs:708` and `:786`; `herd-runner/src/root.rs:1100`. Ruled out and not members: `herd-hub/tests/durability.rs:1107` (a file, not a directory), `herd-hub/tests/killed_hub.rs:192` and `:490`, `herd-hub/src/store.rs:4918`, `herd-runner/src/spool.rs:463` (all `0o500`, which lists fine). | batch 26's red gate, 2026-10-30; D-NEXT | herd-step | owed | Give each loud member the premise guard of `a_failed_listing`, or state in its own words why it cannot retake | D-NEXT | `history/2026-10-30-settle-owed-scan-red.md`; job `j0000000147` |
| ORCH-NEXT+1 | **A fixture whose premise dissolves and whose test stays GREEN, which is worse than tonight's red.** `herd-runner/tests/confinement.rs::a_job_cannot_hide_what_it_wrote_from_its_disk_budget` runs `chmod 300 h; sleep 6`, so its premise is exposed for six seconds against a 200 ms walk and is restored with near-certainty before the inner runner measures. It then asserts `StoppedByCap { Disk }`, which holds either way: through Herd it no longer exercises the defect it is named for, and it would stay green with `restore_access` removed from the product. Same shape with sub-millisecond windows, so a small hole in the wrong direction rather than a hollow test: `herd-runner/src/root.rs::a_directory_a_job_made_unreadable_is_still_measured`, `herd-runner/src/root.rs::a_workspace_a_job_locked_shut_is_still_reclaimed`, `herd-runner/src/outputs.rs::a_directory_the_job_closed_is_still_collected`, and `herdctl/tests/exit_code.rs:756`. Found by reading all 41 `set_permissions` sites and every `chmod` in tests and scripts; it is a code reading, and no run was made with `restore_access` removed. | the point-3 audit of batch 26's red, 2026-10-30 | herd-step | owed | Decide how a test proves the runner's own restoration still happens when something else would have done it anyway — a probe that fails if the product did not restore, or a fixture outside the walked tree | ORCH-NEXT | `history/2026-10-30-settle-owed-scan-red.md` |
```

### `docs/orchestration/workflow.md` — a sentence by the directive

**Drafted for a prose step, not for me.** Another agent may be in that
file and prose is a merger's to place; this branch does not touch it.

```markdown
**And a mode a test sets is not a mode it keeps.** Running the suite
through Herd puts every fixture under a job workspace the runner walks
every 200 ms to hold it to its disk budget, and that walk hands itself
read access back on any directory it cannot list
(`herd_runner::root::restore_access`, `set_permissions(mode | 0o700)`)
— which invariant 2 and D166 require of it. So a fixture that depends
on a directory staying unreadable must check its premise at the moment
it judges a measurement, and treat a measurement taken after the
premise was undone as void (D-NEXT). `0o711` where the fixture set
`0o111` is the fingerprint, and a gate run through Herd is for this
class **not** equivalent to one run locally.
```

## Decisions needed

None for Alexa. Two rows are drafted above and both are *how*, not
*what*: nothing here changes the shape of what the product is or
promises. **The release is not affected** — the known-unstable test
named in the notes is unstable for a reason that is entirely in the
suite's fixtures, and nothing was found in the hub.

## Resume point

`herd-merger`, off `settle-owed-scan-red`: carry this branch into the
batch and run a fresh `./scripts/gate.sh` on the new tip. The
integration, D267, ORCH-286 to ORCH-290 and both board checks are done
on `batch-26-integration` and need no repeating. Decide what to do about
the handoff line in finding 4 before the landing date is written.

## Processes

Started and ended by me, all by a PID I held myself:

- Six Herd jobs, each with its own marker in the command line so none
  was coalesced into another's output: `j0000000141` (the build),
  `j0000000142` (20 runs of the gated binary inside a job),
  `j0000000147` (the mode measurement), `j0000000151` (the fixed
  build), `j0000000153` (the five gates, pre-commit), `j0000000155`
  (the five gates on the committed sha, plus the 200 runs). All through
  `/mnt/ssd_980/herd/queue/bin/herdctl --hub ws://127.0.0.1:8791
  --label build --memory-mib 4700`, never bare.
  `./scripts/bench-preflight.sh` was run before each heavy phase and
  was `ok` each time; `HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB` was never
  set or exported.
- Five `perl` restorer processes, each started by my own script and
  killed by the PID that script had recorded, then `wait`ed. No
  `pkill`, no `killall`, nothing killed by name.
- Nothing was signalled that I did not start. PID 662711 (the queue's
  runner), 565152 (our hub), 3006543 and 3021002 (the consumer's) were
  read and never touched.

Build tree: `/mnt/ssd_980/herd/build/settle-owed-scan-red`, **debug
profile with tests, `du -sh` 12G** — a final figure, measured after the
last build, and `du`'s own rounding is the only error in it. Deleted
once the branch was pushed; the exact command and its result are in the
agent's report. **Evidence was moved out of it first**: the gated
binary is kept as
`settle-red-evidence/bin/owed_scan_fails-PRISTINE-045c1bc641b099f5`
and every log, probe and restorer lives beside it in
`settle-red-evidence/`.
