# e42-producer-submit — handoff 2026-10-30

- **Item:** ORCH-148, and the E-42 row of `docs/coverage.md`
- **Agent:** herd-step
- **Branch / commit:** `e42-producer-submit` / see the push, based on
  origin/main `df47266`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

**E-42 is two rules, and only one of them was ever missing.** The brief
asked which, and the answer is measured rather than argued.

1. **The joining path that is live is E-05's deduplication**, and it is
   keyed on a key the hub **derives**, not on the declared field:
   `herd-hub/src/job.rs:817` `dedup_key(command, labels, commit)`,
   length-prefixed, with the declared commit as its third counted field
   since D156. `JobDirStore::submit` (`herd-hub/src/store.rs:2234`)
   looks that key up in `open_work` under the store's one lock and
   joins **any job that is not terminal**, running included. That is
   exactly the sentence observed on our instance: *"a submission joined
   j0000000073, which is running and answers 2 submission(s) now"*
   (`herd-hub/src/server.rs:8548`).
2. **Dedup and coalesce are two rules, not one**, and D75 says so in
   the words the brief suspected: *"Dedup joins; coalescing discards.
   They are two behaviours, not one"*, and *"A dedup join and a
   coalesce drop can both apply to the same submission; dedup is
   considered first"*. Joining a **running** job is correct for dedup
   and forbidden for coalesce; the live path is the dedup one, so the
   observation that started this task is the rule working, not the rule
   broken.
3. **So E-42's dedup half is already on `main`**, and asserted against
   the real binaries: `herdctl/tests/declared_dedup.rs`
   `two_declared_submissions_at_one_commit_are_one_job`, whose own doc
   comment names the case — *"A push hook firing twice is the case this
   exists for"* — beside
   `two_declared_submissions_at_two_commits_are_two_jobs`, which is the
   detector that proves the key is not simply always equal.
4. **What was missing from that half is the detached path**, and this
   branch adds it. Both existing tests go through `herdctl run`, which
   follows its job; a hook goes through `herdctl submit`, which takes
   `JobAccepted` and closes the socket
   (`herdctl/src/lib.rs:743` `submit_job_announcing`). The two are not
   the same code, and nothing asserted the second.
   **New file, the only product change on this branch:**
   `herdctl/tests/detached_dedup.rs`, two end-to-end tests against the
   real `herdctl`, a real hub, a real runner and the four executor
   steps really running:
   - `two_detached_submissions_at_one_commit_are_one_job` — two
     `herdctl submit` of one declared producer at one commit: both exit
     0, **both print the same id**, the record answers 2 submissions,
     and the one execution's own outcome is `exited 0`, which is the
     result both ids name. Sequential and not raced: `submit` returns
     only once the hub has said `JobAccepted`, so the first job's
     record exists by construction when the second leaves, and the
     first job is held inside its run with `--wait-for` so it cannot be
     over.
   - `two_detached_submissions_at_two_commits_are_two_jobs` — the
     detector, on work that must **not** be joined: two commits, two
     jobs, one submission each, identical lines and identical labels,
     so it is the commit and nothing else that made two pieces of work.
5. **ORCH-148 is three fields, not five, and the three are not alike.**
   Measured in the tree at `df47266`:
   - `needs` and `leaves` **now have their wire half**: both are
     fields on `herd_proto::Asked` (`herd-proto/src/lib.rs:2229`,
     `:2242`), added for M11 and citing ORCH-148 in their own doc
     comments. What is still missing for those two is the `herdctl`
     half — `Declared::asked` hardcodes `needs: Vec::new()` and
     `leaves: BTreeMap::new()` (`herdctl/src/lib.rs:390-391`) — and
     that is the step another agent is on.
   - `schedule` **is read**: `herdctl/src/main.rs:1352` calls
     `Resolved::schedule_of`. It is the requester's own predicate by
     D207, so it is carried nowhere on purpose.
   - `coalesce` and `dedup_key` are read by **nothing in production
     code at all**. The only readers in the tree are four assertions in
     `herdctl/tests/declared.rs:200-204,285-286` that the parser parsed
     them. That is the sharpest form the gap takes: the sole consumers
     of those two fields are the tests proving they were consumed.

## Not done, and why

**E-42's coalesce half is not built, and it cannot be built without
editing `herd-proto`** — which is this brief's own stop condition, so it
was stopped on and not worked around. What it would need, each item
measured in the tree:

1. `Asked.coalesce: Option<String>` — `herd_proto::Asked` has no such
   field, and no other frame carries one. `herdctl` has no
   `--coalesce` flag either, so there is no path at all from the
   declaration to the hub.
2. `JobOutcome::Superseded { by }`, exiting 125 — D75 names this
   variant as the design, and `herd-proto/src/lib.rs` has no such
   variant. `JobOutcome::cause` enumerates the nine there are:
   `exited`, `signalled`, `could_not_start`, `stopped_by_cap`,
   `cancelled`, `lost`, `impossible`, `setup_failed`,
   `refused_as_full`.
3. A second index in the store over **queued** jobs only, and a
   supersession under the same one lock `submit` already takes.
4. `herdctl wait` on a superseded job saying which job superseded it
   (D75's own acceptance sentence).

Nothing of 1 to 4 exists anywhere in the tree: `grep -ri
"supersede|coalesc"` over all four crates returns the `herdctl`
declaration parser, the four tests above, and prose about a superseded
*runner connection* or *attempt*, which is a different thing.

**The brief's fact 1 is refuted by the code, and this is the finding
that matters most for sizing:** *"both fields already ride the wire"* is
false, and `docs/road-to-v1.md:100-102` says the same thing in the same
words — *"so this is wiring a field that already exists on the wire"*.
Neither field is on the wire. The work is larger than that sentence
makes it, not smaller.

## Gates

**Run as one Herd job through `/mnt/ssd_980/herd/queue/bin/herdctl`,
`--label build --memory-mib 4700`, after
`./scripts/bench-preflight.sh` cleared** (it refused once at load 10.88
on 12 cores, ceiling 9.00, and was obeyed; it cleared at load 5.91,
21 GiB available, 15251 MiB of slice room). Build tree
`/mnt/ssd_980/herd/build/e42-producer-submit`, deleted once the branch
was pushed.

- `cargo fmt --all --check`, `cargo build --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings` and
  `./scripts/test.sh`, in that order, all four green: job
  `j0000000118`, `test suite: PASSED`. The two new tests ran and
  neither skipped — `test two_detached_submissions_at_one_commit_are_one_job
  ... ok`, `test two_detached_submissions_at_two_commits_are_two_jobs
  ... ok`, `test result: ok. 2 passed; 0 failed; 0 ignored`. Verbatim
  output in the agent's report.
- Skips: **12** `HERD-SKIPPED`, compared **by name** against
  `scripts/expected-skips/local.txt` under `LC_ALL=C` and not by count
  (D187) — the two lists are **identical**, all twelve being the
  cgroup-delegation family. `#[ignore]` census **COMPLETE at 38, 0
  without a reason**. The two new tests add no skip of either kind.
- **A named control, because a green line is not a proof (D27).** The
  same two submissions with one distinct extra argument on the second
  — not the same piece of work, so nothing may join them — asserted
  the real test's own `second_id == first_id` and **FAILED** on it:
  job `j0000000122`, `test result: FAILED. 0 passed; 1 failed`. So the
  detector fires, and the real test's green line carries information.
  The control file was **deleted** and is on no commit; the two files
  the Rust gate ran over are byte-identical to the two that are
  pushed, `herdctl/tests/detached_dedup.rs` at sha256
  `555bb39782b2fddbcf8078336777b6c1afcc847ce3869709179cdab0f520c7ae`.
- Build tree, **debug with tests: 13 GB** at
  `/mnt/ssd_980/herd/build/e42-producer-submit`, deleted once the
  branch was pushed.
- `./scripts/check-docs.sh` is the **last** run, after this file was
  committed and before the push, as `workflow.md` requires; its output
  is in the agent's report and not in the tree it gated.

## Risks and findings

1. **The release body and three documents say E-42 has nothing on
   `main`, and that is wrong in part.** `docs/coverage.md:22-24`
   (*"**2** with none of it (E-14, E-42)"*), `:126`, `:194` and
   `docs/orchestration/board.md:249` and `:576` all count E-42 that
   way, and `docs/road-to-v1.md:100` repeats it. Measured: E-42's
   stated dedup rule — *"two submissions of the same producer at the
   same commit yield one job"* — is on `main`, is E-05 plus D156, and
   has an end-to-end test naming the push hook as its reason to exist.
   What has nothing on `main` is the **coalesce** rule. The honest
   state cell is `part`, like E-12 and E-37, with the coalesce half
   named. **No document was edited on this branch**: the count at the
   top of `coverage.md` is a judgement about the consumer's
   acceptance, this branch may not quietly move a must between
   columns, and the recount is the orchestrator's. It is drafted below
   as a board row instead.
2. **The two rules are not interchangeable at the edges, and a reader
   who equates them will be wrong twice.** The derived key is
   `(command, labels, commit)`; E-42 words its dedup key as
   `produce:<commit>`. For a push hook they coincide exactly — one
   command, one argument list, one label set, the commit varying. They
   diverge for a producer submitted at one commit with **different
   arguments** (two jobs under the derived key, one under
   `produce:<commit>`) and for one submitted with **different labels**
   (two jobs under the derived key, one under `produce:<commit>`). The
   fixture's own producer declares
   `dedup_key = "assemble-{commit}-{args}"`, which agrees with the
   derived key and not with E-42's wording. So the behaviour E-42 asks
   for is delivered and the **field** is still redundant-or-wrong, which
   is ORCH-148's question and not this task's.
3. **A detached submission that joined says nothing about it.** The hub
   sends `JobAccepted` and then a `JobNote` carrying the join sentence
   (`herd-hub/src/server.rs:8543-8570`), and
   `submit_job_announcing` returns on `JobAccepted` and closes the
   socket, so the note goes to a connection that is gone. A hook
   therefore learns it joined only by noticing that the id it was
   handed is an id it already had — which is enough for the documented
   wrapper (`id=$(herdctl submit …)` then `herdctl wait "$id"`) and is
   **not** a sentence saying so. Whether E-42's *"the second says so
   rather than silently vanishing"* is met by returning the existing id
   is a question about what the product promises, not about the code;
   it is in *Decisions needed*. **Nothing on this branch asserts the
   silence**, because asserting a behaviour nobody has chosen would
   freeze it.
4. **`Assigned` is an unanswered case in D75, and it is the one that
   would corrupt a build.** D75 says only a **queued** job is
   superseded and a **running** one never. `JobState`
   (`herd-hub/src/job.rs:223`) has a third state between them:
   `Assigned` — handed to a runner, which has not reported starting it.
   A runner may already have spawned the process when that record still
   reads `assigned`, so superseding an assigned job can discard work in
   flight, which is precisely what D75 forbids. Recommendation, for
   whoever implements it: treat `Assigned` as running and supersede
   `Queued` alone. It is cheap to get right now and expensive to
   discover from a corrupted producer.
5. **Collision risk, named and avoided.** The `coalesce` work would
   need `herdctl/src/declaration.rs` (`Resolved::declared`) and
   `herdctl/src/lib.rs` (`Declared` and `Declared::asked`) — the two
   files the live M11 step owns for `needs`/`leaves`. This branch
   touches **neither**, and touches no `src` file at all: one new test
   file and this handoff. Its two tests also avoid `herdctl wait` on a
   job whose command declares `leaves`, so that they do not become
   coupled to that step's outcome; they read the hub's records instead.
6. **D229 is the fact the decision in item 1 of *Decisions needed*
   turns on, and it cuts both ways.** D229 says in terms that
   `#[serde(default)]` fields on existing variants stay additive after
   `PROTOCOL_VERSION` 3 and do not move the number — so
   `Asked.coalesce` is cheap. The same entry names what is **not**
   recoverable: a `FromHub` variant a *requester* sees. A new
   `JobOutcome` variant travels inside a frame a requester decodes, so
   `Superseded` is of that kind even though it adds no frame. With no
   compatibility to preserve before v1.0.0 that may be entirely
   acceptable — it is a choice, and this task is not the place it is
   made.

## Drafted for `herd-merger`

No `docs/decisions.md` entry is drafted: this task measured and
asserted, and every judgement it uncovered belongs to somebody else.
Two `board.md` rows follow, verbatim.

**Update to the existing `ORCH-148` row** — the row stays **open**, and
these words are appended to its *Next step* cell, after its last
sentence:

```
 **Measured 2026-10-30 on `e42-producer-submit` at `df47266`: it is three fields, not five, and they are not alike.** `needs` and `leaves` now have their wire half — both are fields on `herd_proto::Asked` since M11, citing this row — and what is missing for them is the `herdctl` half, which is the live M11 step. `schedule` **is read**, by `Resolved::schedule_of` from `herdctl/src/main.rs`, and is carried nowhere on purpose (D207: the predicate is the requester's). Only `coalesce` and `dedup_key` are read by nothing in production code: their sole readers in the tree are four assertions in `herdctl/tests/declared.rs` that the parser parsed them. **And the behaviour E-42 wants from `dedup_key` is already delivered without it**, by the hub's derived key (command, labels, declared commit; D156), so the open question for that one field is narrower than the row implies: refuse it at parse time, or carry it and let a declaration override a key that already answers the same question. The two differ only for a producer resubmitted at one commit with different args or different labels
```

**New row**, section *Owed — agreed, not started*, state `open`:

```
| ORCH-NEXT | **E-42 is two rules and the release body counts both as missing.** Measured 2026-10-30 at `df47266`: E-42's dedup rule — two submissions of one producer at one commit are one job — is **on `main`**, is E-05 plus D156's commit-in-the-key, and has end-to-end tests against the real binaries whose own doc names a push hook firing twice as the case they exist for; `e42-producer-submit` adds the two that were missing, over the **detached** `herdctl submit` path a hook actually uses. E-42's **coalesce** rule has nothing anywhere: no `coalesce` on `herd_proto::Asked`, no `--coalesce`, no supersession in the store, no `JobOutcome::Superseded`. So `coverage.md`'s count — *"2 with none of it (E-14, E-42)"* — is wrong about E-42, and `road-to-v1.md`'s *"wiring a field that already exists on the wire"* is wrong twice: neither field is on the wire | `herd-step` on `e42-producer-submit`, 2026-10-30, against the tree and the real binaries | orchestrator, then herd-step | open | **Recount E-42 as `part`, like E-12 and E-37**, with the coalesce half named, and correct `road-to-v1.md`'s sizing sentence — the count at the top of `coverage.md` is a judgement about the consumer's acceptance and an execution agent must not move a must between columns by itself. **Then decide the coalesce half**, which needs `herd-proto` and is therefore a decision and not a step: `Asked.coalesce` is a defaulted field and free of `PROTOCOL_VERSION` by D229, while `JobOutcome::Superseded { by }` is a new variant in an enum a requester decodes, which D229 names as the unrecoverable kind. Before anyone implements it, settle whether `Assigned` is queued or running for D75's rule — a runner may already have spawned the process while the record still reads `assigned`, so superseding it discards work in flight, which is what D75 forbids | ORCH-148 | `herdctl/tests/detached_dedup.rs`; `herdctl/tests/declared_dedup.rs`; `herd-hub/src/job.rs` `dedup_key`; `herd-hub/src/store.rs` `submit`; `herd-proto/src/lib.rs` `Asked`, `JobOutcome`; D75, D156, D229; `docs/coverage.md` E-42 and its counts; `docs/road-to-v1.md`; `docs/orchestration/history/2026-10-30-e42-producer-submit.md` |
```

## Decisions needed

1. **Is E-42's *"the second says so"* met by a detached submit that
   returns the existing job's id and no sentence?** Recommendation:
   **yes, and say so in the row**, because the documented wrapper
   already reads the id and `herdctl wait` on it is the one thing a
   hook does next; a sentence would also have to be read off a socket
   `submit` closes by design. Fallback, if the answer is no: a
   defaulted `joined: bool` on `FromHub::JobAccepted`, which is the
   additive kind D229 permits and still a `herd-proto` edit.
2. **Does the coalesce half go ahead, given that it needs
   `herd-proto`?** Recommendation: **yes, as its own task with a
   decision attached**, sized at `Asked.coalesce` plus
   `JobOutcome::Superseded { by }` plus a queued-only index plus
   `herdctl wait`'s sentence. Fallback: refuse `coalesce` at parse time
   in `herdctl` — a `herdctl`-only change that closes ORCH-148's worst
   half, since a validator that accepts a field nobody reads is worse
   than one that rejects it — and move E-42's coalesce clause to a
   named deviation, as E-41's no-submission skip already is (D217).
3. **Who recounts E-42?** It is an orchestrator judgement by the rule
   this branch obeyed, and it changes a figure Alexa is shown.

## Resume point

Nothing is in flight. Next, in order:

1. Orchestrator: recount E-42's State cell in `docs/coverage.md` and
   correct `docs/road-to-v1.md:100-102`'s *"a field that already exists
   on the wire"*, which no longer matches the tree.
2. Then, if decision 2 above is *yes*: one task for the coalesce half,
   based on a `herd-proto` decision and **not** started before it, and
   not while the M11 step holds `herdctl/src/declaration.rs` and
   `herdctl/src/lib.rs`.

## Processes

No long-lived process was started and nothing was signalled, and no
PID of anyone else's was read or touched. Two Herd jobs, submitted to
our own queue and waited on: `j0000000118`, the four Rust gates, and
`j0000000122`, the named control.
`/mnt/ssd_980/herd/build/e42-producer-submit`, **debug with tests:
13 GB**, deleted once the branch was pushed.
