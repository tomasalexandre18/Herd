# detached-end-event — handoff 2026-10-30

- **Item:** ORCH-181 (its first unbuilt half); E-37, M13
- **Agent:** herd-step
- **Branch / commit:** `detached-end-event` / `a375d3f`, based on
  origin/main `27a1040`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

**E-37's fifth item is built: a detached job's end is announced when its
submitter asked, and never otherwise.**

The asking is one `bool` named `announce_end`, in three places:

- `herd-proto`'s `ToHub::SubmitJob`, beside `asked` and `wants_files`,
  `#[serde(default, skip_serializing_if = "std::ops::Not::not")]`;
- `herd_hub::job::NewJob`, with the builder `announcing_its_end(bool)`
  and `false` from every other constructor;
- `herd_hub::job::JobRecord`, so `job.json`, with the same attributes.

It is added to the submission in `server.rs` as
`new_job(asked).announcing_its_end(announce_end)` and **not** inside
`new_job`, because `new_job` is shared with the dry run (E-07) and a dry
run has no end to announce.

**`herdctl submit --announce-end` is how it is asked for**
(`herdctl::submit_job_announcing`, with `submit_job_saying` now
delegating to it at `false`, so no existing caller changed). `herdctl
run` is offered no such flag at all: it stays and reads the verdict on
its own stream. That is what makes *"detached"* true — the gate is the
asking on the record, never a guess about who happens to be watching at
the moment a job ends, because a follower list is a fact about one
second and E-37 asks what the submitter wanted at submission time.

**The event** is `notify::Event::JobEnded { job, cause, how }`, kind
`job_ended`: `cause` is `JobOutcome::cause()`, the same machine-readable
word the verdict document and the wire use, for a consumer's router to
switch on; `how` is one sentence for a person. They are two fields
because a router must not parse English and a reader must not look the
job up.

**It fires at every door a record can reach a terminal state through**,
through one function, `server.rs`'s `say_how_it_ended`:

1. `record_the_end` — a runner's verdict, first try;
2. the deferred retry loop — a verdict written late;
3. the lost-twice / refused-as-full door, on **both** branches (a job
   refused as full is a different thing to say about the *fleet*, hence
   no `job_lost`, and the same thing to say about the *job*);
4. the requeue-budget-spent door (D76);
5. `cancel_job`'s queued arm — a job cancelled before it started;
6. `end_the_job` — the three ends the claim step decides, which includes
   a nightly cancelled at its 05:00 deadline (E-09, D217).

Each call is after the record is written, so the announcement is about a
fact on the disk. `say_how_it_ended` returns on
`Notifier::configured()` **before** reading the record, so a hub with no
sink pays nothing at all; a hub with one pays one lockless record read
per job ending, beside the manifest read and the commit read those doors
already do. A record it cannot read is said out loud on stderr and
guessed neither way, because the asking is on that record and nowhere
else.

`the_ending_in_words` is a new three-line function beside the existing
`how_it_ended`: that one is written for a *parent that did not succeed*
and says `exited 3 rather than 0`, which is nonsense for a job that
exited 0, and a detached end fires on the green ones too. Every other
variant reads the same either way, so it is that one case and a
delegation — one table of these sentences, not two.

**Neither version number moved, and that judgement is asserted rather
than argued.** `PROTOCOL_VERSION` stays 3 and `RECORD_VERSION` stays 2.
`PROTOCOL_VERSION`'s own doc already names *"a new `#[serde(default)]`
field on an existing variant, when its absence means exactly what its
default says"* as additive, and `RECORD_VERSION`'s sets the same
condition. Both halves of both claims are proved on the **written
bytes**:

- `herd-proto/tests/round_trip.rs`,
  `an_end_nobody_asked_about_is_absent_from_the_wire_and_decodes_as_not_asked`
  — the key is absent from a submission that asks for nothing, present
  as `"announce_end":true` when it asks, and a frame from before the
  field (`{"type":"submit_job","asked":{…}}`) decodes with it `false`;
- `herd-hub/src/store.rs`,
  `a_record_carries_the_end_announcement_its_submitter_asked_for`
  — `job.json` carries `true` when asked, has **no such key** when not,
  and a `job.json` with no such key parses back as nobody asking rather
  than failing. That last assertion is the one the version exists for: a
  record this build cannot parse is the failure mode, and there is none
  here.

**E-37's acceptance criterion is measured, not argued.**
`herd-hub/tests/events.rs`,
`a_detached_jobs_end_is_said_when_its_submitter_asked_and_never_otherwise`
runs **eight detached `herdctl submit`s against a real hub and a real
runner, of which two ask**, and the topic holds **two** `job_ended`
events — three quiet ones before the askings and three after, so the
count follows the number of askings a person typed and not the number of
step jobs the fleet produced. Both askings are asserted on their
contents (`job`, `cause`, `how`, `herd_event_version`, ASCII body and
title), and one of them exits 3: *a detached submitter who is not there
to read exit 3 is exactly who needs telling.*

The existing test
`a_day_of_step_jobs_is_no_events_and_a_runner_coming_and_going_is_two`
was **extended rather than duplicated**, as the brief asked: one of its
day's jobs is now a detached `submit` that asked for nothing, so its
claim stays true now that there is a way to ask.

**The detector is proved, so the zero is a result.** With
`if record.announce_end` replaced by `if true` in `say_how_it_ended` and
nothing else changed, `cargo test -p herd-hub --test events` gives
`test result: FAILED. 1 passed; 3 failed` — the two counting tests above
and `the_owners_own_switch_is_an_event_and_the_hubs_own_asking_is_not`
all go red. The gate was then restored from a byte copy and the target
re-run green (`4 passed`).

The hub's startup line now says **five** things and names
`--announce-end`; `events.rs` asserts both strings, so the sentence an
operator reads cannot drift from the list.

**Docs**: `coverage.md`'s E-37 row and its narrative line, `plan.md`'s
M13 and M14 sections, and the board's ORCH-181 note.

## Not done, and why

- **The nightly's verdict, E-37's last item — deliberately not built,
  not stubbed, and it needs no event of its own.** This is the fact the
  brief asked to have confirmed or refuted by someone who had read the
  code: **confirmed.** A scheduled pass is `herdctl if-moved` submitting
  on the consumer's own timer (D207, D217) — a submission nobody is
  following, which is exactly the case the work above handles. The hub
  side exists and announces **every** ending, the 05:00 deadline
  cancellation included (door 6). What is missing is one line in
  `herdctl`: `main.rs`'s `Action::IfMoved` arm still calls
  `submit_job_saying`, which submits at `announce_end: false`. Pointing
  it at `submit_job_announcing` and giving `if-moved` its own flag is
  M14's and is the whole of it. **So E-37's completion waits on M14 and
  not on more M13 work.** D217's recorded deviation stays a deviation
  and is untouched: a pass that submitted *nothing* is `herdctl`'s own
  stderr line, because the hub never heard of a job and an event about a
  job that does not exist is not a thing this hub can say.
- **`docs/coverage.md`'s E-37 row stays `part`, not `merged`.** Four of
  five became five of five in the hub, but the fifth listed *thing* —
  the nightly's verdict — cannot be demonstrated end to end until M14's
  nightly exists. Promoting a must on an argument is what the row's own
  state word exists to prevent. The musts tally (36 of 44) is therefore
  unchanged by this branch.
- **`deploy/README.md`'s paragraph** about running ntfy in the compose
  network and pointing the hub at it. Owed by ORCH-181 since batch 8,
  outside this task, and left where it was.
- **No `docs/decisions.md` entry written.** That file is
  `herd-merger`'s; the two version judgements are drafted below.

## Gates

All four Rust gates plus `check-docs.sh` were run on the committed tree
(`a375d3f`), from the worktree, with
`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/detached-end-event`. None was
piped into anything that could hide its status; `fmt` and `check-docs`
were run bare and the others had their status read with
`${PIPESTATUS[0]}` (gotcha
`docs/orchestration/gotchas/a-gate-joined-by-a-semicolon-is-advice.md`).
`./scripts/bench-preflight.sh` was run before each heavy phase and
**refused the first attempt** (load 10.74 against a ceiling of 9.00);
nothing was compiled until it cleared.

```
----- GATE 1: build -----
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 9.78s
BUILD_EXIT=0
----- GATE 2: test.sh -----
TEST_EXIT=0
 12 assertion(s) COULD NOT RUN on this host and were skipped.
ignored census: COMPLETE (38 ignored, 0 without a reason)
test suite: PASSED
----- GATE 3: clippy -----
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 8.27s
CLIPPY_EXIT=0
----- GATE 4: fmt -----
FMT_EXIT=0
----- GATE 5: check-docs -----
docs ok
DOCS_EXIT=0
```

**Skips: unchanged, neither list moved.** 12 runtime `HERD-SKIPPED`
assertions — the cgroup-delegation and `io.max` ones this host cannot
carry — and 38 compile-time `#[ignore]`s, census COMPLETE with 0 without
a reason. Both figures match what the brief expected, and no name
entered or left either list (D187). A `HERD-SKIPPED` line is a skip and
is counted as nothing else (D27).

**The last run**, made after this file was committed and before the
push, is `./scripts/check-docs.sh` from the worktree; its verbatim output
is in this agent's report, not in the tree it has just gated.

## Risks and findings

- **The widening, for the merger to look for.** One protocol variant was
  widened: **`ToHub::SubmitJob`** gains a third field. Every struct
  literal of it is updated on this branch — `herdctl/src/lib.rs` (2),
  `herd-proto/tests/round_trip.rs` (4), `herd-hub/tests/follower_alive.rs`
  (2) — and the hub's one `match` arm destructures it. **`JobRecord`**
  and **`NewJob`** each gain a field, so their literals moved too:
  `herd-hub/src/store.rs` (the `mint` destructure and the record it
  builds), `herd-hub/src/job.rs` (`hypothetical`), `herd-hub/src/queue.rs`
  and `herd-hub/src/matcher.rs` (test helpers). Both new fields have a
  `false` default, so a branch that adds *another* literal of any of the
  three will not conflict in git and **will not compile** — the failure
  mode that reddened batches 13 and 14 and `pack-fetch`'s merge. Every
  test crate was compiled here (`clippy --all-targets`, then the whole
  suite) and all three are green.
- **Overlap with `pack-assignment`, which is in flight on
  `herd-hub/src/server.rs`.** This branch touches that file in five
  places, none of them `AssignJob`'s construction or the registry:
  the `ToHub::SubmitJob` match arm (~7925 before, now a four-line
  pattern), the `.submit(…)` call beside it, two new functions added
  immediately before `end_the_job`, and six one-line calls at the
  terminal doors. It does **not** touch `herd-hub/src/registry.rs` or
  `herd-runner/src/lib.rs` at all. A textual conflict is likely only in
  `server.rs`, and resolving it is keeping both sides' lines.
- **A job lost twice can now produce two events**, `job_lost` and
  `job_ended`, and that is deliberate: the first is about a *machine*
  going and is addressed to the fleet's operator, the second about a
  *job* ending and is addressed to whoever asked. Each still fires at
  most once. It is written down in `Event::JobEnded`'s own doc so that
  nobody later reads it as a duplicate and removes one.
- **E-05's joiner does not change the asking.** A second identical
  submission that joins an open job gets that job's id and its record is
  not rewritten, so a joiner who asked for an announcement on a job
  whose first submitter did not ask gets none. One execution has one
  end; the joiner is told how it went on its own connection. Named in
  `JobRecord::announce_end`'s doc rather than left to be discovered.
- **The brief's figures held.** 12 skips, 38 `#[ignore]`s, the
  `keeper_for` precedent and the `RECORD_VERSION` comment were all as
  described; no figure in the brief needed correcting. The brief said
  "a field on `NewJob`, on `ToHub::SubmitJob`, and in `job.json`", and
  the tree's own `notify.rs` comment said the same — both were right.

## Drafted for `herd-merger`

The brief did not ask for a `docs/decisions.md` entry. It did ask for
the two version judgements to be reasoned in the handoff so the merger
can turn them into a decision if they earn one, so the entry is drafted
in full below and the merger may use it or drop it. The number is
`D-NEXT`; the merger allocates it and gaps stay (D148).

```markdown
## D-NEXT — 2026-10-30 — A detached job's end is announced on the
submitter's asking, and neither version moved for it

E-37's fifth item needed the submitter's asking carried from the
submission to the end, because an announcement at the end has to be
asked for at the beginning and nothing on the connection survives a
`herdctl submit` that has already left. **Decided:** one `bool`,
`announce_end`, on `ToHub::SubmitJob`, on `NewJob` and on `job.json`,
asked for by `herdctl submit --announce-end` and by nothing else;
`herdctl run` is offered no such flag, which is what makes *detached*
true. The gate on emission is that field on the record and **not**
whether anybody is following the job: a follower list is a fact about
one second, and E-37 asks what the submitter wanted at submission time.
The event, `job_ended`, fires at every door a record can reach a
terminal state through — a verdict on either try, lost twice, refused as
full, out of requeues, cancelled while queued, and the three the claim
step decides — because *a detached job's end* is the end, and an
announcement that only fired on success is the one that would let a red
nightly pass unnoticed.

**Neither `PROTOCOL_VERSION` nor `RECORD_VERSION` moved**, and that is
the substance of this entry rather than a note on it, because all three
of D199's moves are spent. Both fields are
`#[serde(default, skip_serializing_if = "std::ops::Not::not")]` and
absent when nobody asked, which satisfies the condition each constant's
own doc already sets: a `#[serde(default)]` field whose absence means
exactly what its default says. Absence means *nobody asked*, and that is
true of every frame and every record written before the field existed,
since there was no way to ask. **Asserted on the written bytes and not
argued:** a submission that asks for nothing encodes to the bytes it
encoded to before the field existed
(`herd-proto/tests/round_trip.rs`), and a `job.json` with no such key
reads back as nobody asking rather than failing to parse
(`herd-hub/src/store.rs`) — which is the failure mode `RECORD_VERSION`
exists for, there being no migration path anywhere.

**E-37's criterion is measured:** `herd-hub/tests/events.rs` runs eight
detached submissions against a real hub and a real runner, two of which
ask, and the topic holds two ends. The detector was proved against a
known-bad build — with the record's gate removed, three of that file's
four tests go red — so the zero is a measurement and not a silence.

**What is left of E-37 is the nightly's verdict, and it is this same
event rather than a new one.** A scheduled pass is `herdctl if-moved`
submitting on the consumer's own timer (D207, D217): a submission nobody
is following. So E-37's completion waits on **M14**, not on further M13
work, and what it needs is `if-moved` asking for the announcement on the
submission it makes — one line in `herdctl`. D217's deviation is
untouched: a pass that submitted nothing stays `herdctl`'s own stderr
line, because the hub never heard of a job.
```

No new `board.md` row is proposed. ORCH-181's note cell is amended on
this branch with what was built, what moved no version, and what is left
(the `deploy/README.md` paragraph, still owed). Its state stays `done`.

## Decisions needed

None for Alexa. One judgement is the orchestrator's and is a *how*, so
it is reported rather than asked: **whether `coverage.md`'s E-37 row
should be promoted from `part` to `merged` once this lands.** The
recommendation is **no** — five of five transitions exist in the hub, but
the nightly's verdict cannot be demonstrated end to end until M14's
nightly exists, and a must promoted on an argument is what that state
word exists to prevent. The fallback, if the orchestrator reads the row
as being about *hub capability* rather than *demonstrated behaviour*, is
to promote it when M14's `if-moved` flag lands, in the same batch.

## Resume point

For M14, when it is next in flight, the one line that closes E-37:

```
# herdctl/src/main.rs, Action::IfMoved: submit_job_saying -> submit_job_announcing,
# with a --announce-end flag of its own on `herdctl if-moved`.
grep -n "submit_job_saying" /mnt/ssd_980/herd-project/herdctl/src/main.rs
```

For this branch, nothing: it is green, committed and pushed, and the
next act on it is `herd-merger`'s integration of the batch.

## Processes

PIDs started: none that outlived a command. Every `cargo`, `nix develop`
and test process was a foreground child of a shell this agent ran and
had exited before the next step; none was backgrounded and none was
killed. The first consumer's hub and runner (**3006543**, **3021002**)
were never signalled, inspected or touched; they were waited out, which
is what `bench-preflight.sh`'s first refusal enforced. No `pkill`, no
`killall`, nothing by name.

Build tree: `/mnt/ssd_980/herd/build/detached-end-event`, created by
this task and removed by literal path once the branch was pushed
(gotcha `build-weight-off-root`). The worktree's own `target/` was never
written to.
