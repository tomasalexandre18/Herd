# t1t2-events-detector — handoff 2026-11-01

- **Item:** ORCH-345
- **Agent:** herd-step
- **Branch / commit:** `t1t2-events-detector` / `75ae1a2` plus this
  handoff's own commit, based on origin/main `d6c4048`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

Two parts, one per commit, plus this handoff.

### Part 1 — the detector (`ff89a4d`)

`herd-hub/src/store/frozen_events.rs`, new, declared from
`herd-hub/src/store.rs` as `#[cfg(test)] mod frozen_events;` beside
`ordering_oracle`. Inside the crate because `EventLine` and
`JobDirStore::event_line` are private to `store`.

It is three things in one file, and the third is the one that binds.

1. **Fourteen byte-frozen lines.** Nine, one per `JobEventKind`, and five
   `transition` lines, one per `JobState`. Each is produced by the hub's
   own writer — `JobDirStore::event_line`, the method that puts
   `record_version` in front — and not by a second serialiser written for
   the test. Every literal carries `"record_version":2` and the moment as
   RFC 3339 in UTC with `Z`, which is part of the format.
2. **A decode leg.** The same fourteen lines are hand-written into a
   minted job's `events.jsonl` and read back through
   `JobStore::job_events` (precedent: `herd-hub/tests/crash_points.rs`),
   and the events that come out are asserted equal to the events that
   went in. This is ORCH-345's own *cheap first step*.
3. **Exhaustive matches.** `frozen_line` over `JobEventKind` and
   `frozen_state` over `JobState`, each naming **every field of every
   variant with no `..` and no `_` arm anywhere**. So three different
   changes stop the build in this file: a tenth variant, a field added to
   a variant, and a field taken out of one.

`WHEN_THIS_FIRES` carries the repair instruction into every failure
message, modelled on `herd-proto/tests/frozen_formats.rs:40`-`:49`. It
names three repairs and says which one is always wrong, and it
distinguishes the additive case (a new kind: a frozen const, an arm, a
list entry, `HOW_MANY_KINDS` moved, a sentence in the release notice)
from the non-additive one (`RECORD_VERSION` moves, with `LAYOUT_VERSION`
beside it, and that needs Alexa's approval).

The whitelist at `herd-hub/tests/announced_job_ceiling.rs:396`-`:417` is
retired, and what replaces it is a comment saying why it did not bind and
where the detector is. Neither `BTreeSet` nor `events_of` became unused —
both are used elsewhere in that file.

**Five tests, all green, and my derived bytes matched the code on the
first run.** No expectation was adjusted to match output, which is worth
saying because that adjustment is exactly the failure mode the file is
built against.

### Part 2 — the rule where a reader looks (`75ae1a2`)

D292's extension of `RECORD_VERSION`'s additive rule from *fields* to
*variants* lived only in a decision entry. It is now in four places,
each citing D8 for why the surface is published and D292 for the rule:

- `herd-hub/src/job.rs`, `RECORD_VERSION`'s doc: three paragraphs — the
  variant rule and its ground, what it buys and what it asks of a parser
  outside Herd, and that it is no licence to respell a tag or drop a
  field.
- `herd-hub/src/job.rs`, `JobEventKind`'s doc: a `# Adding a kind here`
  heading, which is where somebody adding kind ten is standing. Three
  things an addition owes, and `#[serde(other)]` refused by name with the
  regression it would cause.
- `docs/architecture.md`, the `RECORD_VERSION` row's *What moves it*
  cell. Its `herd-hub/src/job.rs:72` citation was stale by this commit's
  own insertion and is now `:104`.
- `docs/coverage.md`, E-53's cell.

**No written byte of any record changed**, so Part 1's expectations stay
valid, and no version number moved.

### Board

ORCH-345's action cell is rewritten to **read D8** plus what was done; its
own finding (no in-tree reader, no detector) is kept verbatim as the thing
that closed; *Who* is `herd-step` and the state is `done`. One new row,
literally `ORCH-NEXT`, for the one gap this pass names rather than closes.
Both rows start at column 1 and `board.md:207` was not touched — the diff
is two insertions and one deletion, nothing else.

## Not done, and why

- **T3 and T4 of the sizing pass:** out of scope by the brief. T3 waits on
  a question with the director; T4 (`herd-hub events <job-id>`) needs a
  sizing call from the orchestrator.
- **`#[serde(other)]`:** deliberately not added, pre-refuted by the brief
  and by the sizing pass, and the refusal is now written into
  `JobEventKind`'s doc with its reason.
- **`herdctl history`:** deliberately not built. ORCH-125 already refused
  the class at `herd-hub/src/main.rs:306`-`:318`.
- **`JobOutcome`'s own variants are not frozen.** `JobState::Finished`
  carries one, so a `transition` line nests it; one outcome is frozen as
  the shape of the nesting and the others are not enumerated. Filed as
  the `ORCH-NEXT` row below rather than fixed here, because `JobOutcome`
  is `herd-proto`'s and is read by three parsers, not one.
- **`docs/decisions.md`** not touched: append-only, `herd-merger`'s. The
  entry is drafted below in full.

## Gates

Everything below was run under `nix develop --command`, in this task's own
worktree, with the build tree off `/`:

```text
worktree:         <scratchpad>/wt-events-detector
CARGO_TARGET_DIR: /mnt/ssd_980/herd/build/t1t2-events-detector
```

The scratchpad's own absolute path is in the agent's report; it is this
session's and no later reader can reach it.

**`cargo fmt --all --check`** — run twice, after Part 1 and after Part 2.
Silent both times, exit 0.

**`cargo clippy --workspace --all-targets -- -D warnings`** — green after
Part 1 and again after Part 2:

```text
    Checking herd-hub v1.0.0 (.../wt-events-detector/herd-hub)
    Checking herd-runner v1.0.0 (.../wt-events-detector/herd-runner)
    Checking herdctl v1.0.0 (.../wt-events-detector/herdctl)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 7.36s
```

**The detector itself**, `cargo test -p herd-hub --lib store::frozen_events`:

```text
running 5 tests
test store::frozen_events::every_line_carries_the_record_version_in_front_of_it ... ok
test store::frozen_events::there_are_nine_kinds_and_the_list_holds_all_of_them ... ok
test store::frozen_events::every_kind_of_line_is_this_text_and_no_other ... ok
test store::frozen_events::every_transition_state_is_this_text_and_no_other ... ok
test store::frozen_events::the_frozen_lines_read_back_as_the_events_they_were_written_from ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 308 filtered out; finished in 0.13s
```

**The detector shown firing**, on four known-bad fixtures, each applied to
`herd-hub/src/job.rs` and reverted from a pristine copy afterwards. The
tree was confirmed pristine (`git diff --name-only -- herd-hub/src/job.rs`
empty) before Part 1 was committed, and the five tests were re-run green
after the last revert. Verbatim reds are in the agent's report; in short:

1. **A respelled tag** — `#[serde(rename = "short_collection")]` on
   `CollectionShort`. Two tests red: the frozen text, and the decode leg
   with `unknown variant 'collection_short', expected one of ...`.
2. **A dropped field** — `#[serde(skip)]` on `OutputsRemoved::bytes`. Two
   tests red, and the decode leg read the line back as `bytes: 0` — a
   true number inside a false record, which is the case worth having.
3. **A tenth variant** — `DiskRanOut { free_bytes: u64 }`. **Compile
   error** `E0004` at `frozen_events.rs:206`, *"non-exhaustive patterns:
   `&JobEventKind::DiskRanOut { .. }` not covered"*. This is the fixture
   the retired whitelist was green on.
4. **A field added to a variant** — `since_s: u64` on `Waiting`.
   **Compile error** at `frozen_events.rs:230`, *"missing field
   `since_s`"*, which is what earns the claim that no `..` is used.

**Not run locally: `./scripts/test.sh`.** The whole suite is the gate's
job and the gate is named below.

**The last run, named and not quoted.** The branch's own whole gate, bare
and through the queue, on the tree with this handoff committed:

```text
/mnt/ssd_980/herd/queue/bin/herdctl --hub ws://127.0.0.1:8791 run \
  --label build --memory-mib 4700 -- /bin/sh -c \
  "cd <worktree> && export CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/\
t1t2-events-detector && ./scripts/gate.sh"
```

Its receipt is the job's own record on the queue hub; the verbatim output
is in the agent's report.

**`./scripts/check-docs.sh`** is run after this file is committed; its
output is in the agent's report.

**Skips.** No skip was added. A `#[cfg(test)] mod` adds no `#[ignore]`, so
the ignore list is expected to read **38** names at
`f7a38e4ee5f51977f0a1adbb9afb55aedfb5496268202f6c4744d2aed7a46545` and
the suite's skip list **13** at
`67ae71ed344e1ed18cb6c1c75002aabcc15e77de010c0601d1b89a81c1b20489`, or
**15** at
`c9dd3040103efcedea68c62392b44e653304b933528293d0886f69b29a36222f` if the
two load-gated flood benches are gated out. A sha that moves is a finding,
not a pass. The detector adds **five** tests that run.

## Risks and findings

1. **I ran a heavy phase against a refused preflight, once.** Chaining
   `./scripts/bench-preflight.sh | grep "bench-preflight:" && <phase>`
   continues when the refusal *matches* the grep, because `grep`'s exit
   status is 0 on a match. The brief warned against reading `$?` through
   a pipe and this is the same defect in a different costume. The phase
   was one incremental `clippy` run at load 14.04 against a ceiling of
   9.00; it finished in 7.36 s and nothing else was affected. Every other
   heavy phase was preceded by a verdict read as `ok, safe to start`.
   **The lesson is mechanical: a grep for the verdict line is not a test
   of the verdict**, and anything scripted around this should read the
   verdict's text, not the pipeline's status.
2. **One premise of the brief is slightly too strong, and the correction
   favours the brief's conclusion.** The brief says that when
   `collection_short` landed *"nothing went red"*. Nothing went red
   **about the format** — but `JobEvent::state()` at
   `herd-hub/src/job.rs:2108` already carries an exhaustive match over
   `JobEventKind`, so the ninth variant did force an edit there. It
   forced an arm returning `None`, which is not an expectation about the
   bytes: the compiler asked *"what does this kind do about state?"* and
   never *"what does this kind look like on disk?"*. So the whitelist was
   green, and the existing bind was real and insufficient. Both halves of
   the brief's argument survive; the sentence just needs that clause.
3. **I added a fifth frozen table the brief did not ask for:** the five
   `transition` lines, one per `JobState`, through a second exhaustive
   match. The brief specified nine lines over `JobEventKind`. A state is
   as published as a kind — a sixth `JobState` changes a `transition`
   line's bytes, and `JobState`'s own doc says M9's cancellation was
   expected to add one — so the same defect class applies one level down.
   Reported rather than assumed: if the orchestrator wants the file
   narrowed to the brief, `frozen_state` and `every_state` come out in one
   edit.
4. **The gap I could not close is one level further down**, and it is the
   `ORCH-NEXT` row: `JobOutcome` is `herd-proto`'s, travels on the wire
   under `PROTOCOL_VERSION` and in the verdict document under
   `VERDICT_FORMAT_VERSION`, and freezing it in `herd-hub` would be the
   third place it is pinned. It needs a sizing call, not an edit.
5. **`docs/architecture.md`'s `RECORD_VERSION` row cited
   `herd-hub/src/job.rs:72`**, and Part 2's own insertion moved the
   constant to `:104`. Corrected in the same cell. Nothing in
   `check-docs.sh` validates a `file:line` citation, so every other such
   citation in the docs is unverified by any gate — named here as a
   standing hazard, not as a task.
6. **No `#[ignore]` and no skip was added**, so neither skip sha should
   move. If one does, the detector is not what moved it and the finding
   is elsewhere.

## Drafted for `herd-merger`

### The decision entry, in full

```markdown
## 2026-11-01 — D-NEXT A published record format owes a detector the
## compiler can enforce

`events.jsonl` has been a published compatibility surface since D8, whose
*Reversibility* says it in one sentence: *"The on-disk layout becomes a
compatibility surface the moment the consumer reads it; the record format
therefore carries a version from the start."* The condition is met — the
first consumer parses the file with their own code (E-53, E-54, D292) —
and two other places in the tree already carried the ruling:
`herd-hub/src/job.rs`'s opening sentence, and `docs/handover.md:67`,
which lists the file among what crosses the line in a table its own
preamble calls *"the contract between the two"*. **So nothing here
decides whether the file is published.** What was missing was a detector,
and ORCH-345 was right about that and wrong that the surface was in
question.

**A frozen fixture alone is not a detector, and this project has the
measurement.** `herd-hub/tests/announced_job_ceiling.rs:396`-`:417`
enumerated **eight** event kinds and was green on 2026-10-31 with
**nine** in the tree: `collection_short` landed and nothing went red,
because that list was only ever checked against the lines the jobs of one
scenario happen to produce. A fixture of today's lines says nothing about
kind ten, since nothing about the old lines changed. The same shape has
cost this project three controls in a day: an enumeration that does not
bind is a control silent on the defect it was built for.

**So a detector for a published format is three things in one file, and
the third is the one that binds.** `herd-hub/src/store/frozen_events.rs`
holds a byte-frozen line per kind, produced by the hub's own writer; a
decode leg that hand-writes the same text and reads it back through
`job_events`, so a line is proven readable and not merely proven stable;
and **exhaustive matches** over `JobEventKind` and `JobState` that name
every field of every variant with no `..` anywhere. A tenth variant, a
field added to a variant and a field taken out of one are each a
**compile error in the detector**. The compiler is the part that cannot
be walked past, printed "ok" and forgotten — and it is the part a
fixture, a round trip and a whitelist all lack.

It lives **inside the crate**, as `#[cfg(test)] mod frozen_events;`
beside the D103 ordering oracle, because `EventLine` and the writer that
puts `RECORD_VERSION` in front of a line are private to `store`. Its own
file for the oracle's reason: it is a gate, not one more test of the
store.

**Shown firing before its silence was trusted**, on four known-bad
fixtures: a respelled `event` tag and a dropped field each turned two
tests red, with the dropped field reading back out of the decode leg as
`bytes: 0` — a true number inside a false record; a tenth variant and a
field added to a variant each stopped the build. This is the project's
own rule about a detector and it has earned it three times this week.

**And D292's rule is now written where the person adding kind ten is
standing.** D292 extended `RECORD_VERSION`'s additive rule from *fields*
to *variants*, on the ground that a line's absence has always meant the
thing did not happen; that extension lived only in a decision entry, so
it would have been re-derived. It is now in `RECORD_VERSION`'s doc, in
`JobEventKind`'s doc under *Adding a kind here*, in
`docs/architecture.md`'s version table and in `docs/coverage.md`'s E-53
cell. Each names the two changes that look additive from inside Rust and
are not — a respelled tag, a removed field — because both keep every
round trip in the suite green while breaking a parser that is not built
on our type.

**`#[serde(other)]` is refused, by name, in the doc comment.** It would
change only *our* readers, and the consumer's parser is not built on our
type (D292). On the one product reader that exists it is a
**regression**: an unknown arm turns `note_collection_short`'s dedupe
into a successful non-match, which re-enables the unbounded
duplicate-line case D292 built that one-shot to prevent. Refusing it also
leaves unanswered an unverified question about whether serde permits it
on an internally tagged enum reached through two `flatten`s, which is a
second reason not to ask.

**A renderer is not refused and is not this entry.** ORCH-125 already
ruled the shape at `herd-hub/src/main.rs:306`-`:318`: not `herdctl`,
because a `FromHub` variant a requester can see moves
`PROTOCOL_VERSION` and a requester is the wrong machine. If one is ever
built it is `herd-hub events <job-id>`, judged on support value and not
on compatibility — and the point of this entry is that the format's
guard no longer waits on that question.

Deliberately not in this decision: whether each addition must be named to
the consumer in the release notice, which is a promise about *their*
parser and is the director's; and `JobOutcome`, which `JobState::Finished`
nests and which this pass freezes one of rather than all of, because it is
`herd-proto`'s and is read by three parsers.

No behaviour changed, no record byte changed, no version number moved.
```

Width: every unfenced line of that entry is at or under 79 columns,
checked with the same count `check-docs.sh` uses. **The header wraps onto
a second `##` line**, which is how D290 is written; if `herd-merger`
prefers one line, the title shortens to *"A published record format owes
a detector the compiler enforces"*.

### The board row, in full

Already written into `docs/orchestration/board.md` as the literal id
`ORCH-NEXT`, one line, starting at column 1, immediately after ORCH-345.
`herd-merger` allocates its number. Its subject: a new `JobOutcome`
variant changes what a `transition` line of `events.jsonl` says and no
exhaustive match binds it, with three candidate shapes stated and not
chosen and an instruction to check claim (c) before believing it.

ORCH-345's own row is rewritten in place — state `done`, *Who*
`herd-step`, action **read D8** plus what was done — and its *Depends*
cell says `closes on D-NEXT`, which is the entry above.

## Decisions needed

None from me. One question is already owed to the director and it is the
orchestrator's to put, not mine: the sizing pass's §6 — whether Herd
promises that a new event kind is additive and asks the consumer's parser
to ignore an `event` value it does not know, or whether every new kind
moves `RECORD_VERSION` and costs them a state-root reset. Everything on
this branch holds either way: the detector fires on both policies, and
the doc comments state the policy D292 already chose, which is the
recommendation.

## Resume point

1. Read the agent's report for the four verbatim reds and the gate
   verdict.
2. If the gate is green: the branch is pushed and waits for
   `herd-merger`'s batch. Nothing else on it is owed.
3. Decide risk 3: keep the five `JobState` lines, or narrow the file to
   the brief's nine. One edit either way.
4. Size the `ORCH-NEXT` row (`JobOutcome`), and file T4
   (`herd-hub events <job-id>`) as its own row if it is wanted.
5. The §6 question for the director is still unasked.

## Processes

PIDs started: only `nix develop --command` children of my own shells —
`cargo fmt`, `cargo clippy` and `cargo test` — each waited for and exited
on its own. None killed, none left running, nothing backgrounded. The
four untouchable PIDs were not signalled.

Build tree `/mnt/ssd_980/herd/build/t1t2-events-detector`, off `/`, is
deleted once the branch is pushed; the figure is in the agent's report.
`/mnt/ssd_980/herd/build/link-cut-clock-figure` is another task's and was
not touched.
