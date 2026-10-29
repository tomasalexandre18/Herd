# collect-gap — handoff 2026-10-29

- **Item:** ORCH-163 (proposed; next free id measured against `origin/main`
  `10ff31d`, which carries ORCH-162 and D210)
- **Agent:** herd-step
- **Branch / commit:** `collect-gap` / see the agent's report, based on
  `origin/main` `10ff31d`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Base deviation, said first

The brief said base on `origin/main` **`171c780`**. By the time this task
started `herd-merger` had landed batch 5, so `origin/main` was **`10ff31d`**
and `171c780` is its ancestor (`git merge-base --is-ancestor`, verified).
This branch is based on `10ff31d`. Batch 5 is prose only and touches none of
the code read here. The coordinator confirmed this mid-task.

Measured against `10ff31d`, not taken on report: highest decision in
`docs/decisions.md` is **D210** (`:10632`), so the next free is **D211** —
*not* D210 as the brief said. Highest board id is **ORCH-162**, so the next
free is **ORCH-163**, as the brief said.

## Done

All of it is a reading of our own source plus one built reproduction.
**Nothing of the consumer's volume, machines or logs was read** (invariant 9
/ D161); every figure about `j0000001561` below is theirs as reported.

### 1. Why the resume never completed, and what v0.3.7 does not contain

The resume path is not missing and it is not silent — it fires on **every**
runner join, unconditionally, after registration succeeds
(`herd-hub/src/server.rs:2891`, the one production caller of
`ask_for_the_files_still_owed` at `:4784`). The consumer's own hub log
confirms it fired.

What is missing is on the **runner**, and it is a version fact:

- `herd-runner/src/lib.rs:1674` `files_held` matches on `inner.collections`,
  an **in-memory** map. The only place a spool is registered is the end of
  the job that produced it. A restarted runner holds nothing, so the `other`
  arm is taken and it prints one line to its own stderr and returns `false`.
- The two closings of that leak **both shipped in v0.4.0 and neither is in
  v0.3.7**, measured with `git cat-file`/`git tag --contains`:
  - `herd-runner/src/sweep.rs` (re-adopt the spool at startup) arrived at
    `b250358`, 2026-10-27 — `git tag --contains b250358` → **`v0.4.0` only**;
    `git cat-file -e v0.3.7:herd-runner/src/sweep.rs` → **absent**.
  - the startup reaper's `outputs/` sweep (delete it and name it) arrived at
    `0c14997`, 2026-10-25 — **`v0.4.0` only**.
  - `v0.3.7` is tagged `3da48f5`, 2026-10-24 14:03:27 +0200, i.e. **before
    both**.
- `v0.3.7`'s `files_held` is byte-for-byte today's minus the `heard` flag, so
  the line the consumer's runner journal is expected to carry is exactly:
  `herd-runner: the hub is waiting for the files of j0000001561 attempt 2,
  and this runner holds no collection for it`. Their extraction has since
  confirmed that line, twice on 10-24 and once after the 10-26 reboot.

`docs/remediation/09-runner-self-limits.md` §3 says the leak "is closed".
That is true of the tree and **false of the build the consumer runs**. The
doc names no version, which is how the premise in the brief arose.

### 2. A new finding the brief did not ask for: v0.4.0's fix is defeated

**The startup reaper deletes every spool before the sweep that exists to
re-adopt it can see one.** On `origin/main` today:

- `run_until` → `Confined::prepare` at `herd-runner/src/lib.rs:2633`;
  `Confined::prepare` (`confine.rs:339`) calls `reclaim_a_past_life` at
  `confine.rs:502`;
- `reclaim_a_past_life` (`reclaim.rs:399`) iterates **every** entry under
  `outputs/` with **no guard at all** and `reclaim_directory`s it
  (`reclaim.rs:449-468`); `reclaim_directory` is `fs::remove_dir_all`
  (`root.rs`);
- **29 lines later**, `lib.rs:2662` calls `pick_up_collections` →
  `sweep::pick_up`, which reads the same directory —
  `reclaim.rs:449` is `root.path().join(crate::outputs::SPOOL_DIR)` and
  `root.outputs_dir()` is `self.path.join(crate::outputs::SPOOL_DIR)`
  (`root.rs:205`). Same path, strictly sequential, same process.

Both halves have a green test and **neither runs the startup sequence**:
`reclaim::tests` →
`the_reaper_reclaims_an_output_spool_a_previous_life_left_behind` asserts
the spool is gone; `sweep::tests` →
`a_spool_a_previous_runner_left_is_picked_up_with_its_files`
asserts it is picked up. `herd-hub/tests/spools_resumed.rs` drives a **fake**
runner over a raw socket and never calls `Confined::prepare`.

**Reproduced, deterministically**, by a new test added here:
`herd-runner/src/sweep.rs`,
`the_reaper_does_not_delete_a_spool_this_sweep_would_have_resumed`. It does
what `Confined::prepare` does, in that order, and fails at the first
assertion with the reaper's own sentence. It is `#[ignore]`d so the workspace
suite reports it as a **skip and never a pass** (D27, invariant 5) rather
than going red for a defect nobody has been asked to fix. Run on purpose:
`nix develop --command cargo test -p herd-runner --lib \`
`  sweep::tests -- --ignored`.

Consequence to state plainly to the consumer: **upgrading their runner to
v0.4.0 will not recover `j0000001561`'s 396 MiB.** It will delete the spool
and print one line.

### 3. Why the job is `finished / exited 0` over a 661-file manifest

Two writers, and nothing between them requires the collection to be whole.

- `JobStore::record_outputs` (`herd-hub/src/store.rs:1908`) writes the
  manifest from what the **runner announced on the verdict frame**, before a
  byte of a streamed collection has moved — deliberately, and the doc comment
  at `:1944-1952` says why ("the manifest first, the bytes if they are here").
- The terminal state is written beside it by the ordinary verdict path, and
  `record_metrics` is documented as going first so a reader of the terminal
  state finds the measurements beside it (`:1889-1898`).
- **Nothing anywhere makes `finished` conditional on the collection.** There
  is one definition of complete — `collection_is_whole`
  (`store.rs:554-561`): every manifest entry present in `files/` at the
  manifest's length — and it is consulted by `collection_owed`
  (`:2739-2756`), by the fetch path and by the sweeper. It is **never**
  consulted by anything that writes state.

That is correct as far as it goes: the hub cannot know a manifest is short
until the bytes stop, and D20 forbids a side channel becoming the verdict.
**The defect is not `finished`; it is that the shortfall is never written
down anywhere a reader of the record can see it.** `outputs` has a field for
every other ending — `refused`, `skipped`, `removed`, `first_fetched_at` —
and none for "this collection stopped arriving".

### 4. What a requester actually gets today — the code keeps its promise

The coordinator asked for this to be verified rather than assumed. **Our own
code is right and both earlier readings were wrong**, with one real flaw.

With `--outputs-to`, for each of the 36 files:

- `send_the_files` (`server.rs:4905-4968`) computes `coming` from
  `collections_owed()`. For `j0000001561` that is still `Some` six days on —
  streamed, not whole, not removed, has an attempt — so each absence carries
  `why: "its runner has not finished sending it to this hub yet"` and
  `still_coming: true`.
- `fetch_once` (`herdctl/src/lib.rs:1311-1320`) sets `coming = true` and
  **skips** those absences, so they are not named in that round. Correct by
  design: a line per poll about a working transfer.
- `fetch_missing_outputs` (`:1186-1219`) polls every 200 ms and, 30 s after
  progress stops (`OUTPUTS_STALL_LIMIT`), prints
  *"its runner stopped sending this collection 30s ago, with N byte(s) of it
  here."* and returns `Ok`.
- `write_outputs` (`:904-930`) then prints one line **per missing path**:
  *"did NOT get <path> (N bytes on this job's record): the hub does not have
  it"*.
- `keep_the_outputs` (`herdctl/src/main.rs:1328-1360`) counts off the
  manifest, not the directory, and emits
  *"36 of 661 output file(s) are not here … This run did part of what it says,
  so it does not exit 0."* — and it **does not exit 0** (D167).

So a requester is **told, counted, named, and the command fails**. No short
collection is passed off as whole. Three flaws remain, all small:

1. The reason is wrong where it matters. The stall line and the per-file
   lines say "the hub does not have it"; the hub knows from `collections_owed`
   that it has been owed these bytes for six days and that it has asked ten
   or more times. Nothing of that reaches the requester.
2. `herdctl run` **without** `--outputs-to` fetches nothing, prints
   *"j0000001561 kept 661 output file(s); pass --outputs-to <DIR> to write
   them"* (`main.rs:1305-1312`, `outputs.count()`), and exits 0. 661 is the
   manifest. This is the reading the consumer took.
3. Every poll of that 30 s costs a **full `collections_owed()` walk of every
   job directory** (`server.rs:4905`), ~150 walks per fetch, for a job that
   will never complete.

### 5. Nothing ever cleans an orphan `files.part/`

`clear_output_parts` (`store.rs:2208-2214`) exists and does
`fs::remove_dir_all`. It has **exactly one production caller**:
`server.rs:4274`, inside the `ChunkLanded::Complete` arm. So the part
directory is removed **only when the collection completes** — the one event
that by definition did not happen.

Nothing else reaches it. The sweeper does not: `Budget.bytes`
(`job.rs:1050-1063`) excludes part-written bytes by design ("Part-written
bytes are deliberately outside it"). `retention.rs:783` and `report.rs:408`
only **measure** it. `files.gone/` is the sweeper's directory; `files.part/`
is not.

So it is a leak on every interrupted stream, it is **outside the output
budget** so retention will never notice it, and theirs has outlived six days,
a reboot and ten re-asks. The orchestrator's grep was nearly right: one site
removes it, and that site is unreachable for exactly the jobs that need it.

### 6. The protocol question, answered — and the premise corrected

**Can an existing frame carry "I no longer hold this collection"? Yes, and it
costs no version bump.**

First, two corrections:

- `Lost { reason: String }` at `herd-proto/src/lib.rs:879` is a variant of
  **`JobOutcome`** (enum at `:834`), not of `ToHub`. It is E-21's "the runner
  went silent", an *outcome of a job*. It cannot carry this: the job already
  has a terminal outcome, and overwriting `Finished { exited 0 }` with `Lost`
  would destroy the verdict, which is exactly what D20 forbids.
- A new `ToHub` variant **is** breaking, but **not** because an old hub
  cannot parse it. `ToHub` has `#[serde(other)] Unknown` (`:1733-1734`), so
  it parses fine. It breaks at `relay_from_runner`'s catch-all
  (`server.rs:4468-4472`), which answers any unrecognised runner frame with
  `FromHub::Refused` and then `return Ok(())` — **it drops the connection**.
  The runner redials, re-sends, and is dropped again. The conclusion stands;
  the mechanism is the relay, not serde.

The no-bump carrier is **a new optional field on an existing `ToHub`
variant**, and `ToHub::Health` is the natural one:

- No protocol type in `herd-proto` sets `deny_unknown_fields` (grepped; the
  only uses are in `herd-runner/src/capability.rs`, which is config, not the
  wire), and serde ignores unknown fields by default. An old hub reads the
  `Health` it knows and **ignores** the new field. No refusal, no disconnect.
- The precedent is in that same variant: `load_milli` is
  `#[serde(default, skip_serializing_if = "Option::is_none")]` on
  `ToHub::Health`, added at `135510e`, and that commit **did not touch
  `PROTOCOL_VERSION`** (verified: `git show 135510e -- herd-proto/src/lib.rs
  | grep PROTOCOL_VERSION` → nothing). `PROTOCOL_VERSION` is still 2.
- `Health` is already the frame the resume barrier is built on: the hub
  answers every report with `HealthRecorded`, and `sweep.rs`'s module doc
  explains that the first answer to a report proves the whole owed list has
  arrived. The runner already sends it on a cadence, so there is no new
  round trip and no new liveness path.

**So this fix does not have to ride V29.** It can land whenever it is
briefed. That is the answer to the one question asked.

Worth noting for whoever sizes it: a new `ToHub` variant would, in the
old-hub case, break only a fleet already in this defective state — the frame
is sent **only** in answer to a `JobFilesHeld`, and an old hub that is owed
nothing never sends one. But the field is cheaper and carries no such
argument, so there is no reason to prefer the variant.

### 7. `herd-hub report` — why the consumer needed a shell loop

The per-job comparison is already in hand and the data is already in the same
loop; only the line is missing.

`report.rs:385-390` computes, per job, both `job_bytes` (claimed) and
`measured` (`files/`). It pushes a name in exactly two cases:

- `job_bytes > 0 && measured == 0` → `claimed_but_absent`, printed with
  names at `:797-811`;
- the record says removed and `measured > 0` → `removed_but_present`, printed
  with names at `:813-828`.

`j0000001561` is **neither**: it measures 143 MB, not 0. It lands only in the
gap total — `THE RECORDS CLAIM {gap} MORE THAN THE DIRECTORIES MEASURE`
(`:786-789`) — and is named nowhere. The two named lists cover the two
extremes and miss the middle, which is the defect state.

**Proposal, and it needs no flag** (invariant 1: a knob per question is how a
reporting tool becomes a query language). One third list in the existing
`NAMED_AT_MOST = 10` shape (`report.rs:64`), from data already computed:

```
  claiming more than files/ holds     N job(s)
    jobs/j0000001561: record claims 533.2 MiB, files/ measures 136.4 MiB, short 396.8 MiB
    and N more
```

The condition is `job_bytes > measured && measured > 0`, inside the `if
still_held` arm that already holds `claimed_but_absent`. Two further small
things in the same place:

- **Name the `files.part/` jobs too.** `:407-413` already measures them per
  job and prints only a count and a total; pushing the name is one line.
- **Reword that line.** It says
  `files.part/ still arriving — N job(s), N bytes` (`:830-835`). For a
  six-day-old orphan "still arriving" is false, and it is the sentence that
  would have told the consumer where to look. Something that does not assert
  a live transfer — `files.part/ part-written` — with the names under it.

## Not done, and why

- **No product change**, per the brief. Everything above is diagnosis. The
  one file changed under `herd-*/src` is a test, and it is `#[ignore]`d.
- **Board rows and the decision are proposed, not written.** `board.md` and
  `decisions.md` are untouched on this branch: the board's own convention is
  that rows are written by `herd-merger` at a landing, and this agent may not
  write `decisions.md` except a `D-NEXT` entry it was asked for, which it was
  not. The proposed text is in the agent's report, ready to lift.
- **The hub-side retry/timeout question is answered negative by reading, not
  by a test.** Nothing on the hub notices that no bytes follow a
  `JobFilesHeld`: there is no timeout, no retry counter, no mark on the
  record and no line. `collections_owed()` simply lists the debt again at the
  next join, for ever. A test for an absence of a mechanism would assert
  nothing the reading does not.
- **Mixed-fleet question (their runner 0.3.6 against a 0.3.7 hub):** read and
  judged **not a factor**. `files_held`'s `other` arm is identical in both,
  and the minor-gap path serves with a note. Flagged, not pursued.

## Gates

Run **before** this file was committed, on the tree that is this commit minus
this file, verbatim:

```
########## GATE 1: cargo fmt --all --check ##########
fmt exit: 0
########## GATE 2: cargo build --workspace ##########
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 8.08s
build exit: 0
########## GATE 3: cargo clippy --workspace --all-targets -- -D warnings ##########
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 13.08s
clippy exit: 0
########## GATE 4: ./scripts/test.sh ##########
 12 assertion(s) COULD NOT RUN on this host and were skipped.
 A green run does NOT mean these were checked.
test suite: PASSED
test.sh exit: 0
########## GATE 5: ./scripts/check-docs.sh ##########
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
check-docs exit: 0
```

All four Rust gates were run through `nix develop --command`, with
`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/collect-gap`.

The new test's own line, from `cargo test -p herd-runner --lib`:

```
test sweep::tests::the_reaper_does_not_delete_a_spool_this_sweep_would_have_resumed ... ignored, reproduces ORCH-163; run it on purpose. Red until the reaper stops deleting what the sweep re-adopts
test result: ok. 185 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out
```

**The last run**, on the committed tree including this file:
`./scripts/check-docs.sh`, run bare (not inside `nix develop` — nesting
`TMPDIR` panics `confinement.rs:2529` on `SUN_LEN`). Its verbatim output is
in the agent's report.

**Skips — 13, none of them a pass.** Twelve are the host's pre-existing
cgroup-delegation skips, printed by `test.sh` with a `HERD-SKIPPED` line each
and unchanged by this branch; this host has no delegated cgroup v2 subtree.
The thirteenth is this branch's own
`the_reaper_does_not_delete_a_spool_this_sweep_would_have_resumed`, reported
`ignored` with its reason. **It is red when run** — that is what it is for —
and it is copied here as a skip, never counted as a pass (D27).

## Risks and findings

- **The v0.4.0 reaper/sweep collision (§2) is the largest thing here and it
  was not in the brief.** It means the fix for this defect is in the tree and
  does not work, that the advice "upgrade the runner" is wrong, and that two
  green unit tests are covering a broken composition. Reproduced, not argued.
- **Both of the orchestrator's earlier readings of §4 were wrong in the
  requester's favour**, which is worth saying because the consumer was told
  twice. With `--outputs-to` the path is honest and exits non-zero. Without
  it, exit 0 over a 661-file manifest — which is the reading they took.
- **`files.part/` is outside the output budget**, so a fleet with many
  interrupted streams accumulates bytes retention will never account for or
  free. Theirs is 14 MiB; the bound is the per-job output cap times the
  number of runners, and nothing pretends otherwise (`job.rs:1050-1063` says
  so deliberately) — but nothing cleans it either.
- **Not verified, and it cannot be from here:** every figure about
  `j0000001561` is the consumer's report. The prediction this reading makes
  and that their extraction can still falsify is that the spool directory
  `outputs/j0000001561-2` is **still on their runner's disk**, holding the
  missing 396 MiB, because v0.3.7 has neither the reaper's `outputs/` sweep
  nor the re-adopting sweep. **If so, the bytes are recoverable** — and
  §2 says they must not simply be upgraded into deletion.

## Decisions needed

1. **Is "`finished` over an incomplete collection" a defect or a
   specification choice?** — *Recommendation: a specification choice about
   `finished`, and a defect about the silence.* The hub cannot know a
   manifest is short until the bytes stop, and D20 forbids the collection
   becoming the verdict, so `finished / exited 0` is **correct** and should
   not change. What is a defect, by the project's own standard, is that an
   incomplete collection is recorded as though it were complete:
   `OutputSummary` has a field for every other ending and none for this one.
   D27 prints a skipped assertion as skipped; D179 prints "AT LEAST n" and
   yields a floor. A manifest that says 661 when 625 arrived is the same
   family and should say so on the record. *Fallback if Alexa disagrees:*
   leave the record alone and fix only `report` (§7), which at least makes it
   findable. **This is the one that needs Alexa** — it is a product-surface
   choice, and it is the question their orchestrator handed back.
2. **How many defects?** — *Recommendation: four, and they should be four
   rows, because they have four different owners and three different
   lifetimes.* (a) the hub never records a failed resume and re-asks for ever
   — the centre; (b) the orphan `files.part/` is unreachable garbage; (c) the
   v0.4.0 reaper deletes what the v0.4.0 sweep re-adopts — **the only one
   that is a regression in shipped code and the only one with a reproduction**;
   (d) `report` names the two extremes and not the middle. (c) and (d) are
   independently fixable and cheap; (a) is the design question in 1.
3. **No `D` number is proposed for the protocol finding.** It makes no
   choice — it *removes* a constraint the coordinator thought existed. If
   Alexa wants the "an optional `#[serde(default)]` field on an existing
   frame is not a protocol bump" rule written down rather than re-derived, it
   is **D211** (measured; D210 is taken). Recommend recording it: it is the
   difference between this fix riding V29 and landing next week.

## Resume point

Nothing in this branch is landable work; it is a diagnosis plus one ignored
test. For whoever continues:

1. Put *Decisions needed* 1 to Alexa — one question, with the recommendation
   above. Until it is answered, (a) cannot be briefed.
2. Brief (c) immediately and separately — it does not wait on Alexa, it has a
   reproduction, and it is live in `origin/main`. The fix is a decision about
   which of the two startup halves owns `outputs/`; the reproduction inverts
   to green when it is made. Suggested one-line test of the fix:
   `nix develop --command cargo test -p herd-runner --lib \`
   `  sweep::tests -- --ignored`
3. Tell the consumer two things, in this order: their spool is probably still
   on disk and holds the missing bytes; **do not upgrade the runner before
   (c) lands**, or the upgrade will delete it.

## Processes

PIDs started: `nix develop --command` cargo invocations only (fmt, build,
clippy, `test.sh`, two `cargo test -p herd-runner`), each run in the
foreground and each exited on its own. None left running, none killed, no
`pkill` and no kill by name.

`CARGO_TARGET_DIR` was `/mnt/ssd_980/herd/build/collect-gap`, outside the
worktree and off the root filesystem, removed once the branch was pushed
(`rm -rf`, gotcha `build-weight-off-root`). The worktree's own `target/` was
never written to.

`./scripts/bench-preflight.sh` was run before the first compile and returned
`ok, safe to start` — 336 GiB free, load 0.71 on 12 cores against a ceiling
of 9.00, 22 GiB available. Batch 5 had already landed, so no integration was
in flight.
