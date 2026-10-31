# t1-ask-collection-on-verdict — handoff 2026-10-31

- **Item:** ORCH-NEXT (the merger allocates it; this branch guesses no number)
- **Agent:** herd-step
- **Branch / commit:** `t1-ask-collection-on-verdict` / see the last commit of
  this branch, based on origin/main `7309b16798b45050f17bf9dfe9d1652d4fa68636`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

**The hub asks for a streamed collection the moment it learns there is one.**
In the `ToHub::JobFinished` arm of `herd-hub/src/server.rs`, after
`record_the_end` has written the manifest and the `JobResultAck` has gone out,
the hub now asks `JobStore::collection_owed` for that one job and sends one
`FromHub::JobFilesHeld` when a collection is owed by the runner that spoke.
Guarded on the frame — `outputs.streamed && !outputs.files.is_empty()` — so an
ordinary verdict, which is almost every verdict, reads no record for a
collection it has not got.

It is the owed-collections scan's own derivation asked for one job, D79's one
definition asked a third way, so there is no second rule to keep in agreement
with the scan.

**What it closes.** The scan that was the only starter of a hub-driven transfer
runs at join, *before the hub reads a frame* (its own comment). A verdict a
runner held across a link break therefore writes its manifest after the only
question that would ever have started the bytes, and nothing asks again:
`Session::attach` deliberately does not restart the pump, and the held-verdict
loop only gives up and reports. That is the first consumer's 2026-10-31 loss —
17 files, 42 MiB, under a record reading `finished, exited 0`.

**The detector, observed red on the unfixed tree first.**
`herd-hub/tests/collection_asked_at_the_verdict.rs`, two tests:

- `a_verdict_delivered_on_a_new_link_has_its_collection_asked_for` — a job
  handed out on one link, the link dropped with the verdict unsent, the verdict
  re-offered on the next link with a 17-file streamed manifest. It asserts the
  hub asks on **that same connection**, then sends the bytes there and asserts
  all 17 are on the hub's disk at length, with no second redial.
- `a_verdict_with_no_streamed_collection_is_asked_nothing` — the counterpart,
  so the new frame is not unconditional.

Both deterministic: no clock, no sleep, no cut socket. The barrier is the
suite's existing one — a health report sent after the verdict, since frames on
one link are ordered, so a `JobFilesHeld` the hub did not send before
`HealthRecorded` is one it never sends. A frame-driven peer is used rather than
the runner binary because the runner's half is settled by its own source: for a
collection held *in memory* there is exactly one restarter,
`Session::files_held`, the other (`offer_swept_collections`) filtering
`files.swept`, true only for a collection read off the disk at startup.

## Not done, and why

1. **T2, T3, T4 and T5 of the analysis** — out of scope by the brief. In
   particular **no event is added where a short collection is given up on**;
   that is T2 and owes its own decision entry.
2. **`herd-proto`, `herd-runner`, `herdctl`** — untouched. No frame changed, no
   `PROTOCOL_VERSION` move: `JobFilesHeld` already existed and
   `Session::files_held` already restarts the pump on receiving it.
3. **`docs/decisions.md`, `docs/status.md`, `docs/coverage.md`,
   `docs/orchestration/board.md`, `CLAUDE.md`, `scripts/`** — not edited. The
   decision entry and the four board rows are drafted below in full.
4. **The duplicate-send waste is not fixed here**, by instruction. It is
   drafted as a row with the measurement that exists and the measurement that
   does not.
5. **No `real_herd` fixture with a cut link was built.** The brief named one;
   the hub decision under test does not need a runner process, and the
   frame-driven shape removed every source of flake the brief warned about (the
   workspace walk's duration trigger, a socket cut on a clock). Said as a
   deviation, not as a saving.

## Gates

All four Rust gates plus `check-docs.sh`, run **bare and whole** through the
local queue as `./scripts/gate.sh`, never per step. Verbatim verdict of the run
made on the fix commit `44aaf1cacc575cebac00586d1941b6a4fb96ea15`, before this
handoff and before the comment correction described under *Risks*:

```
step 0: the tree, the sha and the host
  sha:          44aaf1cacc575cebac00586d1941b6a4fb96ea15
  branch:       t1-ask-collection-on-verdict
  host:         servernixos
  nproc:        12
  load1:        4.32
  the working tree is clean: this gate is about 44aaf1cacc575cebac00586d1941b6a4fb96ea15 and nothing else
step 0: the tree, the sha and the host — ok
step 1: check-docs.sh (ci job: docs) — ok (1s)
step 2: check-orchestration.sh (ci job: docs) — ok (49s)
step 3: nix flake check (ci job: check) — ok (2s)
step 4: cargo fmt (ci job: check) — ok (2s)
step 5: cargo clippy (ci job: check) — ok (22s)
step 6: test suite — ok (366s)
step 7: the skips, both kinds (ci job: check, annotation)
  (a) runtime, HERD-SKIPPED: this host could not check it
  observed skip lines: 13 (the count is not the signal; the list is)
  load-gated benches that RAN, and whose assertions were made:
    ran      flood-fifty
    ran      flood-two-fifty
  the skip list matches the expectation exactly.
  (b) compile-time, #[ignore]: nobody is checking it until someone acts
      observed #[ignore] lines: 38 (the count is not the signal; the list is)
      the #[ignore] list matches the expectation exactly, and every
      one of them carries a reason.
step 7: the skips, both kinds — ok
gate: GREEN sha=44aaf1cacc575cebac00586d1941b6a4fb96ea15 steps=0-8:ok host=servernixos load1=4.32 load1-end=4.28 skips=at-rest
```

**Skips, as a list and never a count (D187).** The at-rest list matched
exactly, 13 names, and both load-gated flood benches **ran** — so this run's
expectation is the at-rest one and the `--nocapture` figure is a figure, not a
selector. The `#[ignore]` list matched exactly at 38 names. **This branch's new
fixture adds no `HERD-SKIPPED` and no `#[ignore]`**, so neither
`scripts/expected-skips/local.txt`, `local-under-load.txt` nor `ignored.txt`
moves, and neither sha the brief quoted changes.

**The last run, named and not quoted** — made on the committed tree of this
branch's final commit, after this file was committed and before the push:

```
/mnt/ssd_980/herd/queue/bin/herdctl --hub ws://127.0.0.1:8791 run \
  --label build --memory-mib 4700 -- /bin/sh -c "cd <this worktree> && \
  export CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/t1-collection-at-verdict && \
  ./scripts/gate.sh"
```

Its log and receipt land under `<this worktree>/target/herd-gate/` as
`gate-<stamp>-<sha12>.log` and `<sha>.receipt`. Its verbatim verdict is in the
agent's report to its caller, because a second commit to carry it into the tree
would need a second gate run.

**Earlier runs, quoted because they are the evidence the fix rests on.**

The detector on the **unfixed** tree, commit `e4dcf57` (the fixture alone, the
fix not yet written):

```
running 2 tests
test a_verdict_with_no_streamed_collection_is_asked_nothing ... ok

thread 'a_verdict_delivered_on_a_new_link_has_its_collection_asked_for' panicked at
herd-hub/tests/collection_asked_at_the_verdict.rs:293:9:
the hub recorded j0000000001 as finished with a manifest of 17 streamed file(s) and
never asked its runner for one of them. It holds 0 of 17 on disk, the runner is on
the line, and nothing will ask again: the join scan already ran, no chunk will
arrive to trigger the fallback, and the held verdict will be released naming every
missing path. This is the first consumer's 2026-10-31 loss of 17 files. Frames the
hub sent: [JobResultAck { job_id: JobId("j0000000001") }, HealthRecorded]

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
```

The hub sent exactly two frames for that verdict and neither was
`JobFilesHeld`. The `finished` assertion above it had already passed, so the
record said the job had succeeded while the hub held none of its output. **The
detector is proven on the known-bad tree before its green was trusted.**

The same two tests on the fix commit:

```
running 2 tests
test a_verdict_with_no_streamed_collection_is_asked_nothing ... ok
herd-hub: j0000000001 ended leaving 17 file(s) this hub does not have; asking
a-runner-whose-link-dropped for them on the link its verdict arrived on
herd-hub: j0000000001 has every file of its collection
test a_verdict_delivered_on_a_new_link_has_its_collection_asked_for ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

`herd-hub: … has every file of its collection` is `server.rs`'s own line for a
completed collection, on the connection the re-offered verdict arrived on, with
no second redial — which is the evidence the brief asked for.

## Risks and findings

**1. The brief's fix adds a third sender of `JobFilesHeld`, and the healthy
path measured zero cost — which refutes my own prediction, not the brief's.**

Asked whether a second start re-sends from zero or resumes from the hub's
offset, read off the code and not off the consumer's three lines:

- `Session::files_held` calls `FilesOut::resume_from(held)` **before**
  `pump_files`, unconditionally. `resume_from` sets `sent[i] =
  held_bytes.min(len)` for **every** entry, so a `JobFilesHeld` reporting
  nothing winds the cursor back to the start of the whole collection. It is a
  **rewind**, not a resume-from-offset. That is the wasteful answer, and it is
  what the consumer's offset refusals are consistent with.
- Two concurrent pumps **cannot** duplicate a chunk between them.
  `FilesOut::next` advances `self.sent[index]` under the session's own mutex
  and hands each chunk out once, so a second `pump_files` task interleaves with
  the first and drains one cursor. The cost of a second start is therefore the
  **rewind**, not the second task.
- The waste a rewind can cost is bounded by what is in flight when it lands:
  `FILES_IN_FLIGHT` = 8 (`herd-runner/src/lib.rs:3671`) times
  `OUTPUT_CHUNK_BYTES` = 1 MiB (`herd-proto/src/outputs.rs:120`), so **at most
  8 MiB, once per collection**, and self-correcting — the hub's refusal arm
  answers a bad offset with its real holdings, which winds the cursor forward.

**Measured, because I first wrote the cost into a code comment as a fact and it
was not one.** `herd-hub/tests/outputs_whole.rs` is the healthy path with a
real runner streaming 33 files of about a megabyte each over loopback. Run with
`--nocapture --test-threads=1` on the fix commit: the new ask fired on both
streaming tests (`ended leaving 33 file(s) … asking test-runner`) and the hub
printed **zero** `a file chunk … was not written` lines. On a link whose round
trip is microseconds the rewind lands before anything has been sent, so there
is nothing to re-send. The 8 MiB figure is **reasoned from those two constants
and is not measured**; it needs a link with real latency to appear at all. The
comment in `server.rs` now says exactly that, and the first wording of it was
corrected in a later commit for precisely this reason.

**Whether this change makes the consumer's measured double-start more likely:
no, and it cannot participate in it.** Their double start was
`files_held` (the hub's join scan) plus `offer_swept_collections` (the runner
offering a collection it picked up off disk). `offer_swept_collections` filters
`files.swept`, true **only** for a collection read off the disk at startup; the
collection this fix is about is in memory (`FilesOut::new`, `swept: false`), so
that starter is inert for it by construction. Their path is restart-only and
swept-only.

**And on the connection this fix acts on, the join scan provably cannot also
have asked.** The scan runs before the connection's first frame, so for it to
have named this job the manifest must already have been on the record — and a
record carrying a manifest written by `record_the_end` is terminal, which
`is_current` refuses at the top of this same arm (`Standing::JobIsOver` →
`StaleAttempt`), so the arm is never reached. The two senders are mutually
exclusive there. **One narrow exception, named rather than hidden:** if
`record_outputs` succeeded and `record_state` failed inside `record_the_end`,
the record carries a manifest and is not terminal; the next link's scan would
then ask, and the verdict landing on that link would ask again. One rewind, in
a partial-write window, self-correcting.

**2. Nothing in the suite measures the healthy-link rewind, and that is a gap
this branch does not close.** `grep` over the gate's own suite output found
zero occurrences of the hub's completion line or its offset-refusal line:
the in-process `Harness` tests have their stdout captured on a pass, and the
`real_herd` tests' hub is a separate process whose output is not in that file.
So the gate being green is **not** evidence about the duplicate-send cost; the
targeted `--nocapture` run above is, and only on loopback.

**3. The brief's premise about where to put the fix holds; its hypothesis was
checked for a refutation and none was found.** Senders of `JobFilesHeld` on the
unfixed tree: `ask_for_the_files_still_owed` (the join scan),
`OwedAtJoin::answer_this_offer` (the ORCH-197 per-offer fallback, needs a
chunk) and the offset-disagreement arm (needs a chunk). Runner-side restarters:
`Session::files_held` and `offer_swept_collections` (gated on `files.swept`).
No fourth sender and no ungated restarter, so the task was necessary.

**4. The analysis's note on `attempt_ended` is confirmed and was not treated as
part of this defect.** Nothing was built on it.

## Drafted for `herd-merger`

### The `docs/decisions.md` entry, in full, to paste as written

```markdown
## 2026-10-31 — D-NEXT The hub asks for a collection when the verdict announces it

The hub's standing question for outstanding collections is asked once per
connection, at join, from `ask_for_the_files_still_owed` — and its own
comment says the frame goes out *before the hub reads a frame*, which is
what makes a runner's silence readable as "this hub wants nothing of
yours". That ordering is a race with a verdict a runner held across a link
break. The scan reads a record with no manifest on it, `collection_owed`
answers `None` because `outputs.files` is empty, the hub names nothing, and
the manifest is written milliseconds later by the re-offered verdict. The
runner's pump is hub-driven, `Session::attach` deliberately does not restart
it, and the held-verdict loop only ever gives up and reports — so nobody
ever asks again.

Measured on the first consumer's instance on 2026-10-31, v1.0.0: a link
down for 71 s at host load 172, a job that finished inside that window, a
verdict delivered on the next link, and **17 output files, 42 MiB, never
collected** under a record reading `finished, exited 0`. The requester saw
`herdctl` exit 2 with 17 "did NOT get" lines and the job's own record said
it had succeeded. Their runner was on the line holding every byte, and a
restart brought the whole collection home — 17 files, 43 011 442 bytes,
digests matching their independent copy — which is why this is a missing
request and not a broken transfer.

**So the question is put where it first has an answer**: in the
`ToHub::JobFinished` arm, after `record_the_end` has written the manifest,
the hub asks `JobStore::collection_owed` for that one job and sends one
`FromHub::JobFilesHeld` when the collection is owed by the runner that
spoke. It is the scan's own derivation asked for one job — D79's one
definition, asked a third way — so there is no second rule to keep in
agreement with it, and it makes the join scan's ordering structurally
irrelevant rather than tightening it.

Hub only. No frame changed and no `PROTOCOL_VERSION` move: `JobFilesHeld`
already existed and the runner already restarts its pump on receiving one.

**What it costs, with the measurement and without the qualifier it has not
earned.** A collection on a link that never broke is already being pumped
when this frame is handled, so this is a second start for it: the frame
reports the nothing the hub holds and `FilesOut::resume_from` winds that
runner's cursor back to the start of the collection. The two pump tasks
cannot duplicate a chunk between them — `FilesOut::next` hands each one out
once under the session's lock — so what a rewind can cost is the chunks
already in flight, re-sent and refused by offset, with the refusal carrying
the hub's real holdings and winding the cursor forward again. Measured on
`herd-hub/tests/outputs_whole.rs`, a real runner streaming 33 files of a
megabyte over loopback, twice: **zero** refused chunks, because the rewind
lands before anything has been sent. The window it could cost on a link
with real latency is `FILES_IN_FLIGHT` 8 times `OUTPUT_CHUNK_BYTES` 1 MiB —
8 MiB, once per collection — and that figure is **reasoned from those two
constants, not measured**. It is a transfer cost and not a correctness one,
and it is a row of its own rather than a second change in this one.

Deliberately not in this decision: an event recorded where a short
collection is given up on (a record-format addition, its own row and its
own entry), and anything about the requeue storm.
```

### Board rows, every one of them literally `ORCH-NEXT`

Two branches collided tonight by both guessing a number; these four guess
none. **`herd-merger` allocates all four**, in the order written. Rows go in
the open-work section, eight columns: id, question/subject, origin, owner,
state, next action, dependency, evidence.

```markdown
| ORCH-NEXT | **The hub asks for a streamed collection the moment it learns there is one, so a verdict held across a link break no longer loses its bytes.** The owed-collections scan runs at join, *before the hub reads a frame*, so a record that gains its manifest from a re-offered verdict is one no scan ever asked about; `Session::attach` does not restart the runner's pump and the held-verdict loop only gives up and reports. Measured on the first consumer's instance 2026-10-31, v1.0.0: link down 71 s at host load 172, **17 files and 42 MiB never collected** under a record reading `finished, exited 0`. Fixed in the `ToHub::JobFinished` arm with the scan's own derivation asked for one job. Hub only, no frame change, no `PROTOCOL_VERSION` move. Detector observed red on `7309b16` first — 0 of 17 files, job already `finished`, the hub having sent only `[JobResultAck, HealthRecorded]` | the consumer's raw records, read by `herd-planner` 2026-10-31 (`v1.0.0-held-result-and-requeue-storm.md`, T1); taken the same day by `t1-ask-collection-on-verdict` | herd-step | done | land with the batch; the decision entry is drafted in the handoff, headed `D-NEXT` | nothing | branch `t1-ask-collection-on-verdict`; `history/2026-10-31-t1-ask-collection-on-verdict.md`; `herd-hub/src/server.rs` the `ToHub::JobFinished` arm; `herd-hub/tests/collection_asked_at_the_verdict.rs`; D79, D148, D167, D34 |
| ORCH-NEXT | **Nothing derives, after the 30 s give-up, that a finished job has outputs it never collected — and the distinction *is* derivable, which is what makes this a defect of derivation and not of recording.** Stated precisely, because the overstatement is easy: two hub stdout lines **do** exist (`herd-hub/src/server.rs`, the "its verdict waits for its files" line and the "collection did not finish arriving" line 30 s later), the requester **is** told twice by `JobNote`, and `job.json`'s `outputs.files` holds every path with its length and digest against an empty `files/`, so `collection_is_whole` is false and a reader can tell a job that produced nothing from a job whose outputs were dropped. **What is missing is that nothing ever derives it** after `say_the_collection_is_short` fires: no event, no counter, no report. This is the analysis's T2 | `herd-planner` 2026-10-31, §1.4 and T2, correcting its own brief's *"nothing was ever written down anywhere"*; filed by `t1-ask-collection-on-verdict`, which was told not to add the event | herd-step | owed | One `JobEvent` where `say_the_collection_is_short` fires, carrying the file count, the byte total and the reason. `JobEventKind::OutputsRemoved { files, bytes, why }` is the model and `outputs_removed` the constructor to copy. **It is a record-format addition**, so it owes its own `D-NEXT` entry and it is a thing a consumer's parser sees. Detector first: that fixture produces exactly five events today and none about files — assert the count before the change | ORCH-NEXT (T1), landed | the analysis T2; `herd-hub/src/job.rs`, `herd-hub/src/store.rs`, `herd-hub/src/server.rs`; D148, D167 |
| ORCH-NEXT | **The requeue storm: a runner's "already running this job" refusal is charged to the capacity budget, and the hub's only throttle is *guaranteed* inert on that branch.** `StillRunningThisJob` shares a match arm with `AtItsCeiling` and `PastItsMemorySum` in `herd-runner/src/job.rs`, so all three are sent as `at_its_ceiling: Some(running)` and charged against `CEILING_REFUSALS_ALLOWED` = 8. The hub's one guard (`herd-hub/src/matcher.rs`, `out_there >= at_once`) is reachable only *below* the announced count, because `take_job` answers `AtItsCeiling` first whenever `running >= most_at_once` — so the throttle is inert exactly when the refusal is guaranteed to repeat. Measured on the consumer's instance: `holding 1` against `jobs_at_once` 2, 8 refusals in 0.51 s, `RefusedAsFull`, exit 125, no verdict. **`herd-proto/src/lib.rs` asserts in prose that the two refusals are *"the same fact … the same consequence"*, and the measurement refutes it: that prose is now known false** and is the sentence to revisit. A genuine ceiling refusal reports `holding == most`, fires the guard and is self-limiting; this one reports `holding < most` by construction | `herd-planner` 2026-10-31, §3.1–§3.3 and T3/T4; filed by `t1-ask-collection-on-verdict`, which was told not to fold it in | herd-planner, then herd-step | owed | T3 and T4 together, or T4 first: a `#[serde(default)]` field on `ToHub::AttemptAbandoned` naming which refusal it was, and an exclusion keyed on `(job, runner)` answering `Admission::ShortNow` until the runner's next health report no longer names that job. No `PROTOCOL_VERSION` move — the D181/D182 precedent is asserted in `announced_job_ceiling.rs`. **Removing the merge from `herd-proto`'s prose changes a written design decision and wants a `herd-reviewer` pass.** Whether the self-collision keeps a budget at all once the offer rate is bounded is the thing to decide in the brief, not during the work. Its test needs ORCH-330's cheap half (a `Policy` the hub re-reads while up), which is its fourth occurrence | ORCH-330's cheap half; `herd-proto`, `herd-runner` and `herd-hub` all move, so the consumer's runner restarts | `herd-runner/src/job.rs`; `herd-hub/src/matcher.rs`; `herd-hub/src/server.rs` `give_the_job_another_go`; `herd-proto/src/lib.rs` the two-refusals paragraph; ORCH-330; D181, D182 |
| ORCH-NEXT | **A green test asserted the violated property and the production job still died, and what has no test anywhere is the *rate*.** `a_job_refused_past_the_bound_ends_and_names_it` (`herd-hub/tests/announced_job_ceiling.rs`) drives the budget to exhaustion with `ceiling_refusals_allowed: 1`, asserts the job ends `refused_as_full` with its figures named, and **explicitly tolerates** the self-collision branch — so the suite asserts that a job refused past the bound dies, has asserted it on the consumer's exact branch, and is green. **The suite is not missing a test of the behaviour; the behaviour is the defect.** Two further reasons the suite could not have caught it: its detector `ceiling_refusals` filters on `"this runner holds at most"`, which the self-collision's sentence does not contain, so that counter would stay 0; and the test releases its stale holder as soon as one refusal is seen, so no stale holder in the suite ever outlives more refusals than the budget allows. **Nothing anywhere asserts a bound on how many times the hub may offer one job to one machine in one second** | `herd-planner` 2026-10-31, §4.1–§4.5; filed by `t1-ask-collection-on-verdict` | herd-step | owed | A test of the rate, not of the ending: a runner announcing 2 slots holding one job the hub has written off, the stale holder **not** released, asserting (a) the job does not reach a terminal state before the holder is released and (b) at most one such refusal per health cadence, counted off `events.jsonl`'s reasons. It needs a counter keyed on the self-collision sentence rather than on `ceiling_refusals`, and it needs ORCH-330's cheap half. **Report this to Alexa with no question attached**: it decides how much a green gate is allowed to mean | the T3/T4 row above; ORCH-330's cheap half | `herd-hub/tests/announced_job_ceiling.rs`; `herd-runner/src/confine.rs` `A_RUNNERS_OWN_CEILING`; `herd-runner/src/lib.rs` the two refusal sentences; D27, D179 |
```

## Decisions needed

**None blocking.** One thing to put in front of whoever sizes the next step, as
a fact and not a question: this fix introduces a second start of the file pump
on the healthy path, whose cost measured **zero** on loopback and is **reasoned
and unmeasured** at up to 8 MiB once per collection on a link with real
latency. The alternatives that would avoid a second start entirely all need new
hub state (a per-connection identity on `live_attempts`, which would hold an
`Outbox` clone in a map and touch disconnect detection) or a clock, and either
is a larger change than this one on a 1.0.1 candidate. Recommendation: ship
this shape and let the row above carry the transfer cost. If the cost is
judged unacceptable before a 1.0.1: measure it first on a latent link, because
nothing has measured it yet.

## Resume point

Push done, nothing owed on this branch. For whoever continues:

1. `herd-merger` lands it with the batch, allocates the `D-NEXT` number and the
   four `ORCH-NEXT` ids, and pastes the entry and the rows as written above.
2. The next bounded task is the analysis's **T2** (the second row above) or
   **ORCH-330's cheap half** as the precondition of T3/T4. T2 is independent of
   both and is the smaller one.
3. T3 and T4 are one branch or T4 first, and neither folds into this one.

## Processes

No background process started and none killed; no PID was signalled. Every
heavy phase ran as a job on the local queue, one at a time, after
`./scripts/bench-preflight.sh` answered `ok` on its own invocation.

Own build tree `/mnt/ssd_980/herd/build/t1-collection-at-verdict` — off `/`, as
`build-weight-off-root` requires — is removed once the branch is pushed; the
report to the caller says so. `/mnt/ssd_980/herd/build/link-cut-clock-figure`
is another agent's and was not touched.
