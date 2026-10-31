# t2-collection-short-event — handoff 2026-10-31

- **Item:** ORCH-341
- **Agent:** herd-step
- **Branch / commit:** `t2-collection-short-event` / see the agent's
  report for the sha, based on origin/main `3ea839c`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

**A collection the hub gives up on is now named on the job's own record.**
One `JobEvent`, written where `say_the_collection_is_short` fires.

What the defect was, stated as the row states it and not louder: the fact
was already *derivable* before this change and nothing derived it. Three
things existed — the hub's two stdout lines with real figures in them, two
`JobNote`s to a requester whose log the first consumer had already purged,
and `job.json`'s manifest naming all 17 paths with lengths and digests
against an empty `files/`. So a job that produced nothing and a job whose
outputs were dropped were two *distinguishable* states of one record
reading `finished, exited 0`, and no event, no counter and no
reconciliation ever told them apart. Both durable witnesses were stdout,
which is not a record.

The change, in the five files it touches:

- `herd-hub/src/job.rs` — `JobEventKind::CollectionShort { files_here,
  files_owed, bytes_here, bytes_owed, runner, why }`, modelled on
  `OutputsRemoved` in its shape, its serde form (`#[serde(tag = "event",
  rename_all = "snake_case")]`, so the line reads `"event":
  "collection_short"`) and its doc-comment discipline; the constructor
  `JobEvent::collection_short`, copied from `outputs_removed`; the new
  arm in `JobEvent::state()`, which is the one exhaustive match on the
  kind; `pub struct CollectionShortfall`, the five figures; and the trait
  method `JobStore::note_collection_short`.
- `herd-hub/src/store.rs` — `note_collection_short` on `JobDirStore`:
  **written once ever** per job, the one-shot taken off the history
  itself rather than off a new record field, so this adds one event kind
  and no second record-format change. Not through `open_for_writing`,
  for `note_outputs_fetched`'s stated reason — that gate refuses every
  write to a job that is over, and a collection can only be given up on
  after the job is over. Nothing of the state, the outcome, the metrics
  or the manifest is touched.
- `herd-hub/src/server.rs` — `how_far_the_collection_got` now returns
  `CollectionShortfall` instead of a `String`, with
  `how_far_it_got_in_words` building the sentence from it, unchanged
  word for word; `TheCollection::StoppedShort` carries the figures beside
  the sentence; `say_the_collection_is_short` writes the event first and
  then says the same thing out loud and by `JobNote`.
- `herd-hub/src/lib.rs` — `CollectionShortfall` exported, because three
  integration tests implement `JobStore`.
- `herd-hub/tests/{cost_surface,junction_breakdown,unsynced_write_cost}.rs`
  — one delegating method each, in each wrapper's own idiom (`charge`,
  `around`), and one in `server.rs`'s own test store.
- `herd-hub/src/store/ordering_oracle.rs` — **step 18**, which the D103
  ordering oracle required and which the first gate run was red for. See
  *Gates* and finding 9: the oracle refused the eleventh event appender
  until it was driven, and driving it forced the one design statement this
  change had left implicit — that this is the **first event appender with
  no record write of its own** to be bound to, because what it records is
  a fact *about* a record that was already durable. `EVENT_APPEND_SITES`
  10 → 11, the oracle's line census 13 → 14, and the second call asserted
  to write nothing, so the one-shot is proven in the lib suite as well as
  in the fixture.
- `herd-hub/src/store.rs` — `readmit_to_the_budget`'s doc comment amended,
  and the same sentence in the oracle beside step 17. Both said *"a new
  variant would make the file undecodable to a hub rolled back"* as the
  reason that path writes no line; the **fact** is unchanged and is now in
  *Risks* finding 2, but as a sentence it read as a rule freezing the
  variant list, and this branch grows it. Both now rest on the reason that
  actually holds there: the `outputs_removed` line already on the history
  stays true, so there is nothing for a second line to say.

**The two judgements the brief asked me to make, and why.**

*One event, at the give-up only, and nothing where the verdict starts
waiting.* The waiting state is the ordinary path: every job with a
streamed collection that takes longer than zero to cross is held, and
almost all of them then complete, so a `waiting` line would be written
for the common case and made redundant by the next line. Worse, the
crossing sentence is **restated on the liveness cadence** — a sixty-second
wait is four notes — so an event there would either multiply or need its
own one-shot. The give-up is the opposite: rare, final, and the one fact a
reader cannot recover. One line is also less to parse, which the brief
preferred. What a reader loses is the ability to tell a *held* verdict
from a *released* one off the history alone — but that is already in
`job.json`'s state and in the absence of this line.

*Counts and not paths.* `OutputsRemoved`, the model, carries `files`,
`bytes` and `why` and deliberately does not repeat the manifest: *"They
are all still on the record; none of them is on the disk."* The same
holds here, and more strongly — the paths are in `outputs.files` in the
**same job directory**, with lengths and digests, which is more than this
line could carry. The collection D167 was measured on had 657 files; that
line would have been 657 paths, in a file whose value is one greppable
line per thing that happened. A reader does have to join two places. What
the counts buy is that the reader knows *whether* to join at all, and
that was the missing derivation, not the paths.

**Evidence, detector first.** The new fixture is
`herd-hub/tests/collection_short_on_the_record.rs`: two tests, 0.55 s,
**no clock, no sleep, no cut socket**. Copied from
`collection_asked_at_the_verdict.rs`: a frame-driven peer, so a
`real_herd` fixture with a cut link — and the ORCH-244 / ORCH-321 /
ORCH-324 wall-clock flake family — is not reintroduced.

`COLLECTION_SILENCE` is 30 s and is the one real clock on this path, and
the fixture does not wait it out, because it is not the only road to the
give-up. `where_the_collection_stands` bounds the wait on the **runner**
first, in its own words *"the runner is the first bound, and it needs no
clock"*: a machine that is not connected is sending nothing, so there is
no silence to time. So the fixture builds that premise instead of timing
it — the peer delivers a verdict announcing 17 streamed files, sends not
a byte, and goes away — and the give-up is then reached by a **request**
(a late `herdctl wait --outputs-to`, answered off the disk) rather than by
a timer. Holding that requester's verdict is the barrier that proves the
write happened: `say_the_collection_is_short` runs before the verdict goes
out. **ORCH-330's cheap half was not needed and is not blocked by this.**

Red on `3ea839c`, verbatim from the panic: *"the hub gave up on
j0000000001's collection and no line of its history says so. It holds 0 of
17 announced file(s), the record reads `finished` …"*, and the history it
prints is **four** lines — `queued`, `assigned`, `attempt_started`,
`finished` — not one of them about files. Green after the change: one
`collection_short` line, `files_here` 0 of `files_owed` 17, `bytes_here` 0
of `bytes_owed` 1088, `runner` the one that held them, `record_version` 2,
and the `why` on the record is the very sentence the requester was given —
asserted by `notes.iter().any(|note| note.contains(why))`, which is what
makes "one derivation, two readers" a tested claim and not a design
intention. A second reader asking the same question writes **no second
line**. The counterpart test drives the same runner and the same manifest
with the bytes following the ask, and asserts the line is **absent**, so
the write is not unconditional.

**The record, as a parser sees it.** Measured, from a probe run whose
`eprintln!` was removed before anything was committed (the committed
fixture has no such line):

    {"at":"2026-10-31T22:46:20.010384439Z","bytes_here":0,
     "bytes_owed":1088,"event":"collection_short","files_here":0,
     "files_owed":17,"record_version":2,
     "runner":"a-runner-that-held-a-collection",
     "why":"0 of 17 output file(s) and 0 bytes of 1088 bytes have reached
     this hub from a-runner-that-held-a-collection, and
     a-runner-that-held-a-collection is not connected to this hub"}

That key order is the **test's** reader, not the file's: `serde_json::Map`
is a `BTreeMap` here (no `preserve_order` feature in `Cargo.lock`), so it
sorts. On disk the order is the declaration order — `record_version`,
`at`, `event`, then `files_here`, `files_owed`, `bytes_here`,
`bytes_owed`, `runner`, `why`.

`docs/architecture.md` §4.1 gains the kind in its list of what
`events.jsonl` carries, plus a paragraph on the two judgements above.
`docs/orchestration/board.md`: ORCH-341 set to `done`, that row only.

## Not done, and why

- **`docs/decisions.md` — untouched, deliberately.** Only `herd-merger`
  appends (invariant 8). The entry is drafted in full below.
- **`RECORD_VERSION` does not move**, and that is a decision rather than
  an omission; the warrant is in the drafted entry and in *Risks*.
- **No `herd-proto`, `herd-runner` or `herdctl` change**, and no
  `PROTOCOL_VERSION` move: this is a record, not the wire. `herdctl`'s
  verdict and its absences are byte for byte what they were.
- **The requeue storm (ORCH-342), the rate bound (ORCH-343) and the
  healthy-link rewind** are untouched, as the brief required.
- **`CLAUDE.md`, `docs/status.md`, `docs/coverage.md`** untouched.
- **No `herdctl` way to read a job's history.** Out of scope, and it is
  the subject of the `ORCH-NEXT` row below.

## Gates

A change in `herd-hub/src` runs all four Rust gates plus
`check-docs.sh`. `cargo` only inside the flake.

`cargo fmt --all --check`, run before this file was written, exit 0 and
no output:

    (no output)

`cargo clippy --workspace --all-targets -- -D warnings`, same tree, with
`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/t2-collection-short-event` —
the one the gate reuses — **19 s** wall:

    Checking herd-proto v1.0.0 (…/herd-proto)
    Checking herd-runner v1.0.0 (…/herd-runner)
    Checking herd-hub v1.0.0 (…/herd-hub)
    Checking herdctl v1.0.0 (…/herdctl)
        Finished `dev` profile [unoptimized + debuginfo] target(s) in 18.69s

The targeted fixture, before and after, same target dir:

    # on 3ea839c, with the source change stashed
    test a_collection_given_up_on_is_named_on_the_record ... FAILED
    test a_collection_that_comes_home_whole_is_not_called_short ... ok
    test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; \
      0 filtered out; finished in 0.51s

    # with the change
    test a_collection_given_up_on_is_named_on_the_record ... ok
    test a_collection_that_comes_home_whole_is_not_called_short ... ok
    test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; \
      0 filtered out; finished in 0.55s

`./scripts/check-docs.sh` on the first commit: `docs ok`, exit 0.
`./scripts/check-orchestration.sh` on the same tree: **350 passed, 0
failed, 1 skipped** — the skip is the pre-existing one, *"the real tree's
newest landing record carries it: 2026-10-31-merge-batch-35.md predates
the **Previous run:** field, required of landings dated 2026-11-01 or
later"*. Named as a skip and counted as a skip, never as a pass (D27).

**The first gate run was RED, and this is what it was red for.** Run on
`ad2d030`, which was the first commit of this branch, through the queue:

    step 6: test suite — FAILED (exit 101, 349s)
    gate: RED sha=ad2d0304cec7e2c9735ee06038974d414679a347 \
      failed=6:test-suite skips=at-rest

One test, and it was the right one:

    ---- store::ordering_oracle::\
    no_writer_of_a_record_or_an_event_was_added_without_the_oracle_seeing_it
    assertion `left == right` failed: store.rs no longer has 10 event
    appenders. D103 binds every one of them to the record write beside it;
    a new one has to be driven by the ordering oracle above before this
    number moves.
      left: 11
     right: 10

Steps 0 to 5 and 7 were green on that sha — `check-docs`,
`check-orchestration`, `nix flake check`, `fmt`, `clippy` (21 s), and the
skips, which matched **exactly**: 13 at rest, both load-gated flood
benches **ran**, and 38 `#[ignore]` names with the lists matching their
expectations. So nothing about the skip shas moved, and the suite's 307
other lib tests and every integration binary passed.

The number was **not** moved to make it green. `note_collection_short`
was driven through the oracle as step 18 — the thing the oracle's own
message asks for — and only then did the count move. The oracle's two
tests after that:

    test store::ordering_oracle::\
      every_transition_fsyncs_the_record_and_its_directory_before_\
      appending_its_event ... ok
    test store::ordering_oracle::\
      no_writer_of_a_record_or_an_event_was_added_without_the_oracle_\
      seeing_it ... ok
    test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; \
      306 filtered out; finished in 0.26s

`cargo fmt --all --check` and `./scripts/check-docs.sh` were re-run on
the corrected tree before this file was committed; their output is in the
agent's report.

**The last run, named and not quoted** — it happens on the committed tree,
after this file is committed and before the push, so its output cannot be
in the tree it gates:

    /mnt/ssd_980/herd/queue/bin/herdctl --hub ws://127.0.0.1:8791 run \
      --label build --memory-mib 4700 -- /bin/sh -c "cd \
      <this worktree> && export CARGO_TARGET_DIR=\
      /mnt/ssd_980/herd/build/t2-collection-short-event && \
      ./scripts/gate.sh"

Its console output and its receipt go to the agent's report, verbatim,
with the gated sha, the skip list **by name**, and the two shas
(`skip-list-sha256` and `ignore-list-sha256`).

**Skips.** This branch adds **no** `#[ignore]` and no skip: the two new
tests are plain `#[tokio::test]`s that run. So the expected lists are
unmoved — 13 names at rest
(`67ae71ed344e1ed18cb6c1c75002aabcc15e77de010c0601d1b89a81c1b20489`), 15
with the two load-gated flood benches
(`c9dd3040103efcedea68c62392b44e653304b933528293d0886f69b29a36222f`), and
`ignore-list-sha256`
`f7a38e4ee5f51977f0a1adbb9afb55aedfb5496268202f6c4744d2aed7a46545` over
38 names. The gate run's own figures are in the agent's report and are
what decides it; these are the values this branch expects to see, named
here so a move is visible rather than argued.

## Risks and findings

1. **One in-tree comment declined to add an event variant on exactly the
   ground this change crosses, and the reader should know I crossed it
   knowingly.** `JobDirStore::readmit_to_the_budget` says, in its own
   words: *"**No `events.jsonl` line.** `JobEventKind` is `#[serde(tag =
   "event")]` with no `#[serde(other)]` arm, so a new variant would make
   the file undecodable to a hub rolled back."* That sentence is still
   **true** of my variant. What differs is the value of the line it was
   written about: that one would have been redundant (the mark it
   describes lives on the record, and the `outputs_removed` line already
   on the history stays true), while this one is the **only** derivation
   of a loss nothing else performs. I left the comment as it stands,
   because it is a reason for that path and not a rule freezing the
   variant list — but a future reader could read it as the latter, and
   this paragraph is the answer if they do.
2. **What a v1.0.0-era reader sees, in three cases, because they are not
   the same answer.**
   - A v1.0.0 **hub binary** rolled back onto a state root carrying this
     line: **unaffected in normal operation.** `RECORD_VERSION` is 2 in
     v1.0.0 and 2 here, so the root is not refused and no record is
     quarantined; and nothing in the hub, the runner or `herdctl` reads
     `events.jsonl` back — the file has no in-product reader at all
     (checked: the only caller of `JobStore::job_events` outside tests is
     the one-shot this change adds). An older hub appends to the file
     beside the line and never decodes it.
   - A reader that deserializes lines into `herd_hub::JobEvent` as the
     v1.0.0 crate defines it — which is what `job_events` does, and what
     any tool built against that crate would do: **a hard error, not a
     skipped line.** `#[serde(tag = "event")]` with no `#[serde(other)]`
     arm fails on `"event": "collection_short"`, and `job_events` fails
     the **whole call** for that job, naming the line number. One
     unknown line makes that job's whole history unreadable to such a
     reader. That is the real cost of this addition and it is stated
     here rather than discovered.
   - The **first consumer**, who parses `events.jsonl` themselves and
     quoted six of its lines to us with `record_version` dropped:
     whether an unknown `"event"` value is ignored or an error is a
     property of *their* parser and not of ours. A reader that filters
     on the kinds it knows ignores it; one with an exhaustive enum
     errors. **So they need telling the kind exists**, and that is the
     one sentence to send them: *the hub now writes a
     `{"event":"collection_short", …}` line on a job whose streamed
     collection it gave up on, carrying `files_here`/`files_owed`,
     `bytes_here`/`bytes_owed`, `runner` and `why`; `record_version`
     stays 2 and every other line is unchanged.*
3. **`record_version` does not move, and here is the warrant rather than
   the assertion.** `RECORD_VERSION`'s own doc says a bump is *"a
   compatibility statement: it needs Alexa's approval, and it ships with
   either a migration or an explicit instruction for the operator"*, and
   that what stops being readable is *"the operator's whole state
   root — every queued job, every verdict the consumer has not collected
   yet"*. The check it gates is in `read_stored`, on `job.json`, and is
   about records and not event lines. Refusing an operator's entire
   queue to announce one additive history line is out of proportion to
   what the line costs, and the brief grants no such approval. **But the
   letter of that doc does not cover this case**, and I will not pretend
   it does: it says only a field that is `#[serde(default)]` *and* means
   its default when absent is additive enough to leave the number alone.
   That is about record **fields**; a new **event kind** is not
   mentioned, and this is the first one added since `RECORD_VERSION`
   reached 2. The ruling I am taking — and the reason it is a decision
   entry — is that an event kind is additive in the same spirit: the
   line's **absence has always meant the thing did not happen**, which
   is exactly true of every record written before today.
4. **The one-shot has a benign race, named rather than hidden.** The
   test that asks whether a `collection_short` line already exists runs
   with no lock held, because `job_events` takes the store's lock and it
   is not reentrant. Two callers interleaving between the read and the
   append would write the line **twice** — a duplicate on a history,
   not a wrong figure. What the one-shot is for is the unbounded case: a
   line per late `herdctl wait --outputs-to`, for ever, which it does
   prevent. Making it exact would need a record field and a second
   record-format change in the same branch, which is a worse trade.
5. **The board row's own figure was off by one and I am correcting it,
   not quietly.** ORCH-341's *Next action* says *"that fixture produces
   exactly five events today"*. Measured on `3ea839c` it is **four**:
   `queued`, `assigned`, `attempt_started`, `finished`. There is no
   `running` transition, because a frame-driven peer never reports the
   job running, and no `waiting` line, because the job was placed on the
   first pass. The row's claim came from the analysis's expectation of a
   different fixture shape, and the Evidence cell now carries the
   measured four.
6. **`events.jsonl` has no reader inside the product**, which is a fact
   worth keeping beyond this task: `herd-hub` writes it, nothing in
   `herd-hub`, `herd-runner` or `herdctl` reads it back, and `herdctl`
   offers no way to see a job's history at all. So every consumer of the
   record format is **outside** the project and no in-tree reader can
   validate a change to it. That is why this addition is cheap, and it
   is also why the next one will be judged by somebody we cannot test
   against. One `ORCH-NEXT` row below.
7. **The ordering oddity of §1.5 showed up in the detector run**, which
   is a check on the analysis rather than a defect: `assigned` stamped
   `…764336431` was written before `attempt_started` stamped
   `…764264956`, 72 µs apart, in that run. Two clocks, one file. Nothing
   in this change reads the file's order, and nothing should.
8. **Nothing in the brief was contradicted by the code**, with the
   exceptions at findings 5 and 9. The row and the brief agree on what to
   build.
9. **The brief's write-path list was short by one file, and the D103
   ordering oracle is what found it.** The brief and the row both named
   `job.rs`, `store.rs` and `server.rs`; a new `self.append_event(` site
   in `store.rs` also obliges
   `herd-hub/src/store/ordering_oracle.rs`, and the first gate run went
   red on exactly that. **That is the oracle working**, and it is the
   strongest single piece of evidence in this branch: the tripwire fired
   on the day a writer was added, with the reason in its message rather
   than a bare number, which is what its own doc comment says it is for.
   Driving it also forced a statement the change had left implicit —
   `note_collection_short` is the **first event appender with no record
   write of its own**. D103 binds every event append to the record write
   beside it, and there is none here, because what the line records is a
   fact *about* a record that was already durable: the terminal state and
   the manifest were written long before the hub gave up. The two
   log-gap appenders are the precedent for an event-only step, so the
   shape is not new; being event-only *by having nothing to order
   against* is. Nothing of the state, the outcome, the metrics or the
   manifest is touched, which is what keeps `open_for_writing`'s gate
   meaning what it means.
10. **The one-shot is now proven twice, in two suites.** The fixture
    asserts a second *reader* writes no second line; the oracle asserts
    a second *call* returns `Ok(false)` and that the history holds 14
    lines and not 15. The second is the stronger of the two, because it
    is inside the lib suite and does not depend on the server path.

## Drafted for `herd-merger`

### The `docs/decisions.md` entry, in full, to paste as written

Replace `D-NEXT` with the allocated number; every line is inside 79
columns at any number, since `D-NEXT` is the longest form of that token.

```markdown
## 2026-10-31 — D-NEXT A collection given up on says so on the record

The hub's giving up on a streamed collection was said in three places and
written down in none of them. It printed two lines on its own stdout, with
real figures in them; it sent the requester two `JobNote`s; and the
record's own manifest held every missing path with its length and digest
against an emptier `files/` — so the loss was *derivable* and **nothing
derived it**. No event, no counter, no reconciliation. Both durable
witnesses were stdout, which is not a record, and the third was a client
log.

Measured on the first consumer's instance on 2026-10-31, v1.0.0: a job
recorded `finished, exited 0` whose 17 output files and 42 MiB never
arrived. D167 released the held verdict after `COLLECTION_SILENCE` naming
every missing path, which is D167 working as specified; what no artefact
of ours ever said is that it had happened. A nightly reading the record
sees a green job. That gap — between a derivable loss and a derived one —
is the difference between a green nightly and a silent 42 MiB.

**So the derivation is performed once, where the hub gives up.**
`say_the_collection_is_short` writes one `JobEventKind::CollectionShort`
on the job's own history: files here of files owed, bytes here of bytes
owed, the runner, and the same sentence the requester was given.
`OutputsRemoved` is the model in shape, serde form and reasoning.

**One event and not two.** Nothing is written where the verdict *starts*
waiting. That is the ordinary path — every streamed collection is held
while it crosses, almost all of them complete, and the crossing note is
restated on the liveness cadence — so a line there would be written for
the common case and made redundant by the next one. The give-up is rare,
final, and the one fact a reader cannot otherwise recover.

**Counts and not paths**, which is `OutputsRemoved`'s own ruling and for
its reason: the paths are in `outputs.files` in the same job directory,
with lengths and digests, and the 657-file collection D167 was measured
on would have put 657 paths into one line of a file whose value is one
greppable line per thing that happened. A reader joins two places; what
the counts buy is knowing *whether* to join, which is the derivation that
was missing.

**Written once ever** per job, off the history itself rather than off a
new record field, so this is one event kind and not two format changes.
The give-up is reached when a verdict is first held and found hopeless,
again from the held-verdict loop after `COLLECTION_SILENCE`, and again
from every late `herdctl wait --outputs-to` on that job for ever after.
A line per reader would be a history that grows with its readers.

**`RECORD_VERSION` does not move, and this entry is where that is
decided.** A bump is a compatibility statement needing Alexa's approval
and an operator instruction, and what it costs is the operator's **whole
state root**: every queued job and every uncollected verdict stops being
readable to a rolled-back hub. The check it gates is on `job.json`, not on
event lines. Against that, the line costs a rolled-back hub nothing in
normal operation — `events.jsonl` has **no reader inside the product**;
`herd-hub`, `herd-runner` and `herdctl` only ever append to it. The
letter of `RECORD_VERSION`'s own rule speaks of `#[serde(default)]`
*fields* and not of variants, and this is the first event kind added
since that number reached 2, so the ruling is stated rather than assumed:
**an event kind is additive because a line's absence has always meant the
thing did not happen**, which is exactly true of every record written
before today.

What it does cost is named, not softened. `JobEventKind` is
`#[serde(tag = "event")]` with **no `#[serde(other)]` arm**, so a reader
that decodes lines into the older type fails on this one — and
`JobStore::job_events` fails the *whole call* for that job, naming the
line. There is no compatibility to preserve (the consumer resets at
v1.0.0), so this is not a question for anyone; it is a thing to tell the
first consumer, who parses `events.jsonl` themselves. Whether their
parser ignores the kind or errors on it is a property of their parser.

**It is the first event append with no record write beside it**, and D103
binds every other one to the record write it accompanies. There is nothing
here to bind it to, and that is the point rather than an omission: the
line records a fact *about* a record that was already durable — the
terminal state and the manifest were written long before the give-up — so
no order between two writes is at stake. The D103 ordering oracle drives
it as step 18, event-only, as it already drives the two log-gap appenders;
it refused the appender until it was driven, which is the mechanism
working on the day a writer was added.

Hub only. No frame changed, no `PROTOCOL_VERSION` move, and `herdctl`'s
verdict and its named absences are byte for byte what they were.

Deliberately not in this decision: anything about the requeue storm or
about the rate at which the hub re-offers a job, and the healthy-link
rewind — each has its own row.
```

### The `board.md` row this task proposes, literally `ORCH-NEXT`

One row, in the open-work section, eight columns: id, question/subject,
origin, owner, state, next action, dependency, evidence. **`herd-merger`
allocates the number** — two branches collided tonight by guessing one.

```markdown
| ORCH-NEXT | **`events.jsonl` has no reader inside the product, so every consumer of the record format is outside the project and no in-tree reader can validate a change to it.** `herd-hub` appends to it from `JobDirStore::append_event`; nothing in `herd-hub`, `herd-runner` or `herdctl` ever reads it back — the only caller of `JobStore::job_events` outside the suite is the one-shot `note_collection_short` added by ORCH-341 — and `herdctl` offers no way to see a job's history at all. Two consequences, opposite in sign: adding an event kind is **cheap**, because no shipped binary decodes the file; and it is **unvalidatable**, because the readers that would break are the first consumer's own, and `JobEventKind` is `#[serde(tag = "event")]` with no `#[serde(other)]` arm, so one unknown line makes a job's whole history undecodable to a reader built on the older type. `RECORD_VERSION` is no guard here: its check is on `job.json` in `read_stored`, and its additive rule speaks of `#[serde(default)]` fields and not of variants | `t2-collection-short-event`, 2026-10-31, measured while adding the first new event kind since `RECORD_VERSION` reached 2 | herd-planner | open | Decide which of two it is, and it is a *what*, not a *how*: either `events.jsonl` is a published interface, in which case it wants a stated compatibility rule for kinds (the candidate is the one D-NEXT above takes: a kind is additive because a line's absence has always meant the thing did not happen) and the first consumer is told of each addition; or it is internal, in which case a reader belongs in the product — `herdctl history <job>` off the hub's own record — and the suite gains the only reader that can go red. **Not both silently.** A cheap first step either way: a test that decodes every line of a real job's `events.jsonl` through `job_events`, which is the detector this class has none of | nothing | `herd-hub/src/store.rs` `append_event` and `job_events`; `herd-hub/src/job.rs` `JobEventKind`, `RECORD_VERSION` and `readmit_to_the_budget`'s *"No `events.jsonl` line"* comment; `docs/architecture.md` §4.1; D148, D152, D167; the D-NEXT of ORCH-341 |
```

## Decisions needed

**None blocking.** One question is raised for `herd-planner` in the
`ORCH-NEXT` row above — whether `events.jsonl` is a published interface
or an internal file — and it does not gate this branch: the brief
settled that there is no compatibility to preserve, so the addition
stands either way. My recommendation is in the row: decide it once,
rather than per addition.

One thing for the orchestrator to **report** with no question attached:
the sentence to send the first consumer is in *Risks* finding 2, third
bullet. They parse `events.jsonl` themselves, so they are the only
reader this change can surprise.

## Resume point

The branch is pushed and the gate's verdict is in the agent's report.
Next: `herd-merger` lands it with `origin/prove-link-cut-detector`
(`2bcf7fd`, prose only, closing ORCH-321) as one batch — expect adjacency
in `docs/orchestration/board.md` and nowhere else. The merger allocates
the `D-NEXT` above and the one `ORCH-NEXT` row, and writes the landing's
own `Next` item.

Then ORCH-342 and ORCH-343 remain `owed` from tonight's analysis, both
behind ORCH-330's cheap half.

## Processes

No background process started, no PID. `herdctl` jobs submitted to
`ws://127.0.0.1:8791` are listed by their job ids in the agent's report;
each ran to its own verdict and none was killed.

Own build tree `/mnt/ssd_980/herd/build/t2-collection-short-event`,
deleted once the branch was pushed. The 1.9 GiB tree at
`/mnt/ssd_980/herd/build/link-cut-clock-figure` is not mine and was left
alone, as the brief required.
