# merge-batch-8 — handoff 2026-10-29

**The biggest batch this project has landed: thirteen branches, one
integration branch, merged one at a time, one bare gate on the integration
tip, one `--no-ff` merge commit whose message is the landing record, one
push. No tag — no version branch is in it.**

`main` was at `5d422bf`. Twelve branches were named in the brief and a
**thirteenth, `sweep-names-why`, was offered mid-merge and taken**; the
judgement is under *The thirteenth branch* below.

Worktree:
`…/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/wt-batch-8`,
branch `batch-8-integration`.

## Where the things that cannot be in this file are

This handoff is the **last change to the tree**, because the gate must run
on the tree that is pushed (ORCH-121 applied to the merger; `workflow.md`,
*What a landing is*). So these happen after it was committed and are in the
merger's **report to the orchestrator**, not here:

1. the gate's **verbatim output** and its **skip list** — never a skip
   count (D27, D179);
2. the **merge commit sha** and the resolved **gated sha**, which is that
   commit's second parent;
3. the **post-push `ci.yml` run id** on `main`;
4. what step 10 did to the orchestrator's checkout at
   `/mnt/ssd_980/herd-project` — the shas it moved between, or which
   precondition stopped it.

## The batch, in merge order

Order chosen so that `preflight-cgroup` lands first and is then used, and
so that the two branches sharing `herd-hub/src/server.rs` are adjacent.
**Twelve of the thirteen are based on `cd0f1b7`**, the commit before batch
7; only `sweep-names-why` is based on `5d422bf`. That matters twice below.

| # | Branch | At | Clean? | Files |
|---|---|---|---|---|
| 1 | `preflight-cgroup` | `22f0e47` | clean | `scripts/bench-preflight.sh`, `scripts/check-orchestration.sh`, one gotcha |
| 2 | `orch170-policy` | `d0cf65e` | clean | `docs/disk-policy.md` (958), `plan.md`, `coverage.md` |
| 3 | `v5-operating` | `164119b` | clean | `docs/operating.md` (805), new |
| 4 | `claude-md-direction` | `632e199` | **resolved** | `CLAUDE.md` only |
| 5 | `v9-formats` | `478ec79` | clean | `herd-proto`, `herdctl`, two new test files |
| 6 | `v18-status` | `b598cbf` | clean | `herd-hub/src/status.rs` new, `server.rs`, `main.rs`, `lib.rs`, `real_herd/mod.rs`, `architecture.md` |
| 7 | `v20-events` | `4b27290` | **resolved** | `herd-hub/src/notify.rs` new, `server.rs`, `main.rs`, `lib.rs`, `Cargo.toml`, `coverage.md` |
| 8 | `v22-schedule` | `f92c077` | clean | `herdctl/src/moved.rs` new, four `herdctl` files, `scheduling.md`, `coverage.md` |
| 9 | `v10-quarantine` | `3230e59` | clean | `herd-hub/src/store.rs`, `crash_points.rs`, `durability.rs` |
| 10 | `orch168-split` | `b9bcb90` | clean | `herd-hub/src/retention.rs` and its test |
| 11 | `orch166-report` | `211e5df` | clean | `herd-hub/src/report.rs`, `lib.rs`, `state_root_report.rs` |
| 12 | `v23-janitor` | `cabcdfa` | clean | `herd-runner/src/janitor.rs` new, five runner files, `Cargo.lock`, `runner-confinement.md` |
| 13 | `sweep-names-why` | `37a17bc` | clean | `herd-runner/src/sweep.rs` only |

**Eleven merged clean; two were resolved.** Four files are touched by more
than one branch: `herd-hub/src/lib.rs` (3), `docs/coverage.md` (3),
`herd-hub/src/server.rs` (2), `herd-hub/src/main.rs` (2). The first two
auto-merged; the last two are the conflict log.

**Decisions added by the branches themselves: none.** No branch touches
`docs/decisions.md`, `docs/orchestration/board.md` or `docs/status.md` —
all thirteen left those to the integrator and proposed their text in their
handoffs instead. That is why the forward board check below is vacuous and
had to be replaced by something stronger.

## Conflict log

### 1. `CLAUDE.md`, one hunk, branch `claude-md-direction`

**What each side said.** `HEAD` (that is, `main` through batch 7) had the
body of *When to ask Alexa*: the old enumeration, including the word
**"issues"** and batch 7's two additions. The branch deletes that section
and renames it *Direction, and what reaches Alexa*, with the three-outcome
triage and the first duty.

**Kept: the branch's body, whole**, as its own handoff decided after
reading batch 7's diff and writing against the **landed** text rather than
against `cd0f1b7`.

**Why nothing of batch 7 is lost, measured and not assumed.** Both of batch
7's additions survive as triggers in outcome 1 — *"a release and its notes
(not its tag)"* at line 125 and *"anything on the consumer's side"* at line
126 — and the two bullets batch 7 cared about are byte-identical to
`origin/main`'s: the push-to-`main` bullet and the tag bullet. The only
`Git and main` bullet that differs is *One landing per batch*, compressed
from four lines to three by the branch, deliberately and declared.
`diff` of the resolved file against `origin/claude-md-direction:CLAUDE.md`
is **empty**: the resolution is exactly the branch's file, which is what
"whole" has to mean to be checkable.

**150 of 150 lines, longest line 79 columns.** No slack, as before.

**The word "issues" is gone, on the orchestrator's judgement and not the
merger's**, and it is written down rather than re-litigated: a ticket that
changes scope is caught by *anything structural*, one on the consumer's
repository by *anything on the consumer's side*, and the rule itself lives
in `workflow.md`. Recorded in **D214** and as **ORCH-191**, `done`.

### 2. `herd-hub/src/server.rs`, ten hunks, `v18-status` against `v20-events`

Nine of the ten are the same shape: `struct State`'s field declarations and
its **eight** struct literals — one real, seven under `#[cfg(test)]`, which
is the count `v20-events`'s own handoff predicted. `v18` adds
`status_root`, `status_changed`, `status_published`; `v20` adds `notify`.
**Kept: the union, in declaration order**, for all nine. No field of either
side is dropped, renamed or reordered relative to its own branch.

The tenth is `serve_until`'s housekeeping list and is the only one with any
structure to it:

- `v18` adds a fifth `tokio::spawn` inside the `vec![…]` and closes it
  `]);`, the vector being passed straight into `Housekeeping(…)`.
- `v20` closes it `];`, then pushes the sink's task conditionally and
  wraps afterwards: `if let Some(deliveries) = self.deliveries.take() { … }`
  then `let _housekeeping = Housekeeping(housekeeping);`.

**Kept: `v18`'s spawn inside the vector, then `v20`'s close and conditional
push**, so the guard holds both and a dropped future still cannot leave a
task alive on the state root's `flock` — which is the property the comment
above it exists to defend.

**Three comment counts were right for each branch alone and wrong once
merged**, and the merger corrected them: *leave five tasks alive* → **six**,
*The fifth, and the only one* → **the sixth**, *with the other four* →
**the other five**. Comments only, no code. They are named here because a
reader diffing a branch against `main` will find exactly these three lines
changed and should know who changed them and why.

### 3. `herd-hub/src/main.rs`, one hunk, same two branches

`v18` chains `.publishing_status_to(args.state_dir.clone())` onto
`Hub::bind_with(…)`; `v20` calls `Hub::bind_with_sink(…, sink)`. **Both
features must be on**, and the composition is exact rather than chosen:
`bind_with` *delegates* to `bind_with_sink(…, None)` (`server.rs:615`), both
return `Result<Self>`, and `publishing_status_to` is a `self -> Self`
builder. **Kept:**
`Hub::bind_with_sink(…, sink).await?.publishing_status_to(…)`, with `v18`'s
three-line E-35 comment carried onto it unchanged.

`nix develop --command cargo check -p herd-hub --all-targets` is clean on
the resolution, run before the merge was committed; the whole suite is the
gate's job.

**No conflict in this batch changed a specification or an invariant**, so
none was escalated. Every resolution is a union, a composition of two
builders, or a branch's own declared decision.

## D numbers allocated

**D213 to D218, in merge order, from the six branches that proposed one.**
Appended at the end of `docs/decisions.md`; nothing renumbered, no existing
entry edited. The highest on `main` was **D212** and no unlanded branch
holds a number, because no branch touches the file. **Gaps stay: D143,
D154 and D170** — and D170 is a finding of its own, below.

| D | Branch (merge position) | Title |
|---|---|---|
| D213 | `orch170-policy` (2) | Space before age: the spans become a maximum |
| D214 | `claude-md-direction` (4) | Alexa directs; the orchestrator keeps work in flight |
| D215 | `v18-status` (6) | The fleet's state is a document, not a frame |
| D216 | `v20-events` (7) | The events sink: payload, transport, owner's side |
| D217 | `v22-schedule` (8) | E-41's no-submission skip is the requester's line |
| D218 | `v10-quarantine` (9) | An unreadable record is quarantined, not a refused root |

**One proposal refused outright, and the author recommended the refusal.**
`preflight-cgroup` asked whether its 8 GiB cgroup floor and its *an
unreadable cgroup clears and says so* rule want a number, and recommended
no. Agreed: the floor is a threshold in a script, not a promise to the
consumer, and the second half is **D27**'s standing rule already —
something unmeasured is reported as unmeasured. Recorded instead as
**ORCH-177**, with the figure to watch written into the row rather than
into an entry: the `slice room:` line of the first run this check clears
that then goes red anyway.

**One proposal narrowed, and this is the one to read.** `orch170-policy`
asked for an entry titled *"deduplication by default, the spans demoted to
a maximum age"*, and its resume point said to write it *"whatever the
answer to §6 E"*. But §6 E **is** the question *is deduplication on by
default what he has directed*, and the branch's own *Decisions needed*
puts it to Alexa. **D213 records the directed half** — space before age,
D201's spans demoted to a maximum, the budget's documented scope — and
**declines to record deduplication-on-by-default as Alexa's word**, because
a shipped default is irreversible on the consumer's volume and a decision
entry states what he said, not what we expect him to say. The measured
figure it would be decided on is in the entry either way (`r_file` 5.04
against the never-recorded 4), with the day-one-zero cost beside it. Both
§6 E and §6 F go to him as **ORCH-199**, and **ORCH-171's `store.rs` work
waits on E**. That is the one place the merger did not do what a branch
asked, and the brief said to argue any refusal rather than go quiet.

Four branches proposed no entry and none was invented for them:
`preflight-cgroup` (above), `v5-operating`, `v9-formats`, `orch168-split`,
`orch166-report`, `v23-janitor` and `sweep-names-why` — the last three
raised questions that are tasks (ORCH-187, ORCH-186) or host findings
(ORCH-177), not decisions.

## Board ids allocated

**Next free was ORCH-177, measured here and not carried over.** `main`'s
highest is ORCH-176; `orch170-policy` reserves **168 to 175 inside
`docs/disk-policy.md`'s own table**, which a board-only scan misses, and
`orch168-split` reserves 168 to 170 the same way. The scan was run across
`main` and all thirteen branches, over every file and not only the board;
`ORCH-998` and `ORCH-999` appear only as fixtures inside
`scripts/check-orchestration.sh` and were excluded.

**New: ORCH-177 to ORCH-201.** Plus the two ids `disk-policy.md` had
already reserved for work in this batch, **ORCH-168** and **ORCH-170**.
**ORCH-169 and ORCH-171 to ORCH-175 stay reserved and unused** — verified
absent from the board after the edit.

## Gates

### The docs gates, run after every single merge and not only at the end

`./scripts/check-docs.sh` was run **after each of the thirteen merges**,
not once at the end, because one over-long docs line turns exactly 13
`check-orchestration` cases red at once and this batch brought thirteen
handoffs with thirteen chances. It was green every time. Final run:

```
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```

`./scripts/check-orchestration.sh` — **285 passed, 0 failed**, measured
here. That is `main`'s 269 plus `preflight-cgroup`'s 16 and nothing else:
no other branch in this batch adds or moves a case, and `orch162-notes`,
which would have added 6, is not in the batch.

Line ceilings, both measured after every edit, neither with slack:
`docs/status.md` **90 of 90**, `CLAUDE.md` **150 of 150**.

Script modes checked, because an `awk`+`mv` once dropped `100755` and sent
the gate to exit 126: `scripts/check-orchestration.sh`,
`scripts/bench-preflight.sh` and `scripts/gate.sh` are all `100755` in the
index. Every file the merger wrote was written in place.

### The board read both directions, and why the forward read is vacuous

`workflow.md`, *One landing per batch*, says a batch with no conflict needs
this check most. Here it needs a stronger form, because **no branch in this
batch touches `board.md` at all** — all thirteen measured at zero hunks
against `origin/main`. So there is no branch line to find verbatim, and the
forward read proves nothing: the rows were written from the thirteen
handoffs' own proposals, which is what the brief asked.

**Backward**, the honest replacement: the merged `board.md` differs from
`origin/main`'s by the merger's own edit and by nothing else — by
construction, since no branch contributes a hunk to it. A two-dot diff
against each branch shows batch 7's board changes as well, for the one
reason given at the top: twelve of the thirteen are based on `cd0f1b7`.
That is not a merge error and was checked rather than assumed.

**And the check that actually binds, run instead of the vacuous one:** for
every file touched by exactly one branch, the merged tree's version must be
byte-identical to that branch's. **54 of 56 are**, and the two that are not
are both files `main` moved since `cd0f1b7` — `docs/architecture.md`
(`v18-status`) and `Cargo.lock` (`v23-janitor`) — where the merged file is
`main`'s newer version plus the branch's hunk, and **every added line of
both branches is present**.

For the four shared files, every branch's added lines were looked for in
the merged file one by one:

```
herd-hub/src/lib.rs      v18 5/5    v20 2/2    orch166 3/3
docs/coverage.md         orch170 3/3  v20 1/1   v22 1/1
herd-hub/src/server.rs   v18 393/394   v20 185/187
herd-hub/src/main.rs     v18 72/73     v20 57/58
```

**Four lines of 1 405 are not verbatim, and all four are named in the
conflict log**: the three comment counts, and the two constructor lines
that became one. Nothing else of either branch was touched.

### Every branch of the batch left its handoff

All thirteen are in `docs/orchestration/history/`, one per branch, dated
2026-10-29:

```
2026-10-29-claude-md-direction.md
2026-10-29-orch-166-report-names-short-jobs.md
2026-10-29-orch-170-disk-policy-space-first.md
2026-10-29-orch168-split.md
2026-10-29-preflight-cgroup.md
2026-10-29-sweep-names-why.md
2026-10-29-v10-quarantine.md
2026-10-29-v18-status.md
2026-10-29-v20-events.md
2026-10-29-v22-schedule.md
2026-10-29-v23-janitor.md
2026-10-29-v5-operating.md
2026-10-29-v9-formats.md
```

### The mojibake scan, proven before its silence was trusted

`grep` here is ugrep and its `-P` matches no byte pattern in any locale, so
a `perl` byte regex was used — and **proved on a known-bad and a known-good
fixture first**, because a scan that cannot fail is not a scan. The bad
fixture reports 2 and the good one 0. Every file the merger wrote then
reported **0**: `docs/decisions.md`, `docs/orchestration/board.md`,
`docs/status.md`, `CLAUDE.md`, `herd-hub/src/server.rs`,
`herd-hub/src/main.rs`, and this file. The new decision entries decode as
valid UTF-8 and use only `§ · è é — …`.

### The Rust gates, separately

The merger ran `cargo check -p herd-hub --all-targets` on the `v20-events`
resolution and nothing else by hand. `cargo fmt`, `clippy`, the suite and
`nix flake check` are the gate's, below, and are reported from it and not
claimed here.

### The gate, which runs after this commit

**One `./scripts/gate.sh` run, BARE, on a clean tree, and it is the last
thing before the merge.** Not inside `nix develop`: nesting `TMPDIR` panics
`herd-runner/tests/confinement.rs:2529` on `SUN_LEN` (ORCH-139).

The sha it gates is **this handoff commit** — the integration tip, which is
the **second parent** of the merge commit, not the merge commit itself,
whose message is the landing record and which therefore cannot exist yet.
What has to hold at the landing is a GREEN receipt for
`git rev-parse <merge sha>^2` **and** `git diff --quiet` between that sha
and the merge commit: an identical tree is what makes the receipt bind.

The receipt it will write:

```
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/wt-batch-8/target/herd-gate/<gated sha>.receipt
```

with `<gated sha>` the full 40 hex of this commit — a value that cannot be
written here, because naming it would change the tree whose sha it is. The
receipt store is **per worktree** (`$HERD_GATE_RECEIPTS`, default
`target/herd-gate/`) and nothing was ever copied into it. `CARGO_TARGET_DIR`
is set outside the worktree so the build tree can be deleted after the push
while the receipts stay.

**The verbatim output and the skip list go to that receipt and to the
merger's report, not into this tree.** A red step blocks the landing, is
reported red, and `main` does not move.

**Two of the thirteen branches were red on their own gate, and this gate is
what decides them.** `orch166-report` at `211e5df` gated four times — one
green on an earlier sha, two red with every failure a `/chatons.slice`
memory refusal, the last red on a quiet host for the same cgroup reason —
and `v23-janitor` at `cabcdfa` was red for the same cause and re-ran its
five failing targets with headroom for 44 assertions and 0 failures.
Neither red was read as a reason to drop the branch, and **neither
author's word was read as a reason to skip this gate**. ORCH-177, merged
first in this batch, is the fix for the cause.

### The host, checked before the heavy phase

Headroom in `/chatons.slice` was read before the merges began — 14 GiB of
room against an 18 GiB `memory.high`, load1 4.97, 13 GiB free, 335 GB free
on `/mnt/ssd_980` — and `./scripts/bench-preflight.sh`, which carries the
new cgroup check from the moment branch 1 landed, is what gates the gate.
Two sibling tasks were building throughout.

**PID 3046930 is a stray `herd-hub` on `127.0.0.1:18461`** from a task long
finished. It was left exactly as it is and never signalled; no `pkill -f`,
no `killall`, no kill by name was used in this task. **It is worth
reclaiming** — it holds a port and whatever state root it opened, and
nothing in this project will clean it up on its own.

## Not done, and why

1. **The `docs/coverage.md` E-35 and E-40 edits `v18-status` drafted are
   not written.** Its handoff proposes an E-35 note and an E-40 rewording;
   neither is in its diff and neither was added here, because writing a
   coverage row from a handoff's prose is authoring, not integrating.
   Carried in **ORCH-180**'s next action.
2. **`docs/plan.md`'s M13 still says both halves are planned.** One of its
   two musts is delivered. Same reason; same row.
3. **`docs/operating.md`'s status-document section still says *not yet
   shipped*.** It became stale in the other direction inside this very
   batch, which is a shape worth noticing: `v5-operating` and `v18-status`
   were right about the same sentence on the same day. **ORCH-178**.
4. **No tag, and none was owed**: no version branch is in this batch and
   the workspace version does not move. The merger's tag authority (D212)
   is not exercised here.
5. **No pull request.** The one exception dated 2026-10-28 was for the
   landing of the method change and has been used; there is no second one.
6. **`orch162-notes` stays out**, red and never pushed, by its author's
   correct reading of the rule. Recorded as the second sighting on
   **ORCH-162** rather than carried.
7. **The 37.6 GiB was not deleted**, by Alexa's own choice: he waits for
   the version that frees them automatically, because the consumer's volume
   is the only real data that exists and those 75 spools are the only proof
   the fix works on it. Written into **ORCH-192** so nobody tidies them.

## Risks and findings

1. **`check-docs.sh`'s list of known decision gaps is stale.**
   `known_gaps="143 154"`, but `main` is also missing **D170**, so every
   green run — in the gate and in CI's `docs` job — prints
   `note: D170 is a gap …`. A permanent informational note is what trains a
   reader to stop reading them. **ORCH-201.** Gaps stay (D148); this is
   about the note, never about renumbering.
2. **Twelve of thirteen branches were based on `cd0f1b7`, not `5d422bf`.**
   Nothing went wrong, and one branch handled it explicitly:
   `claude-md-direction` wrote its text against batch 7's *result* while
   branching from before it, which is why its one conflict was one hunk and
   not a section. It is a pattern worth repeating when a branch knows
   another is landing into the same lines.
3. **A family, not four notes: the code had the answer and threw it away.**
   `sweep-names-why` found the walk already held the reason for every
   refusal and discarded it at four sites, each turning an `io::Error` into
   a boolean — no new syscall was needed. `v3v4-rates`'s author reports the
   same shape one layer up, in two tests that discard a fact they hold and
   re-derive it from two timings. With **D208** and **D211** that is one
   shape and four sightings in a day. **ORCH-200**, as a review question
   and not as four fixes.
4. **The unit defect is a threshold, not a typo.** `535691 kB` for one job
   in seventy-five, where the other 74 printed bytes: `show_bytes` returns
   the first unit the figure divides **exactly**. One in seventy-five is
   what a divisibility branch looks like from outside. **ORCH-186.**
5. **`v18-status` excluded `status.json` from a test harness's checksum.**
   `herd-hub/tests/real_herd/mod.rs`'s `state_bytes` no longer counts it,
   because a serving hub rewrites it about five times a second. It
   compensated with an **added** assertion rather than by loosening one,
   which is the right trade and is recorded in **D215** so the next reader
   does not re-tighten it.
6. **`v10-quarantine` rewrote a 308-state census from `176 refuse` to
   `0 refuse`.** That reads like a weakened test and is the opposite: the
   old expectations were asserting the defect. The same 308 states are
   still enumerated. **D218** says so, because a future reader will meet
   the diff before the reasoning.

## The thirteenth branch, and the judgement

`sweep-names-why` at `37a17bc` was offered after the first seven merges
were already in, with "no" named as a fine answer. **Taken**, and the
reasoning belongs in the record rather than in a chat:

- **It is uncontended.** `herd-runner/src/sweep.rs` only, plus its handoff.
  Measured here, not accepted on report: it is the only branch in the batch
  touching that file, and `v23-janitor`'s runner files — `reclaim.rs`,
  `confine.rs`, `job.rs`, `lib.rs`, `root.rs`, `janitor.rs` — do not
  include it. It merged clean and `check-docs.sh` stayed green.
- **It is the only branch based on `5d422bf`**, so it carries no
  batch-7-shaped risk at all.
- **The marginal cost is one merge and one set of rows**, because the gate
  runs once over the whole tree either way. Thirteen do not cost more gate
  time than twelve.
- **The marginal risk is one file's compile and tests**, and if the gate
  had reddened on `sweep.rs` the remedy was one `git reset` of one merge
  commit and a re-gate — a cost the merger accepted explicitly rather than
  discovered.
- **What it buys is the consumer's next start.** 50 spools their runner
  refuses to offer, refused with a sentence naming no file and no error, on
  a machine nobody here may log into (D161). Every release it misses is
  another week of them guessing at a hypothesis this one line settles.

Added rows: **ORCH-185** for the change, **ORCH-186** for the defect it
diagnosed on the way past.

## The landing record

Written now, at step 5, because it **is** the message of the `--no-ff`
merge commit of step 8. This copy and that message are the same text. The
gated sha appears in it as "`6c08607` plus this handoff commit, which is
the merge's second parent": exact and self-resolving, and not writable as
40 hex here, for the reason given under *Gates*.

Land batch 8: thirteen branches, from the preflight to the sweep

- **Batch:** 8, thirteen branches, landed 2026-10-29 by herd-merger. One
  integration branch, merged one at a time; one push, one --no-ff merge
  commit carrying this record, one post-push ci.yml run. No tag: no version
  branch is in this batch and the workspace version does not move.
- **Gate:** green -- ./scripts/gate.sh on `6c08607` plus the merge-handoff
  commit, which is this commit's second parent and the last change to the
  tree; trees identical, git diff --quiet between that sha and this one
  clean. Run BARE, not inside nix develop: nesting TMPDIR panics
  confinement.rs:2529 on SUN_LEN (ORCH-139). The receipt is named for that
  sha under target/herd-gate/ in the merger's own worktree, per worktree,
  never copied in. The resolved 40-hex sha, the verbatim output and the
  receipt path are in the merger's report: each exists only after the gated
  tree was closed, so none can be in this message.
- **Skips:** 12, the at-rest names of scripts/expected-skips/local.txt, each
  printed as a skip and none counted into a pass (D27, invariant 5). Named
  one by one under *Evidence*. The list decides, never the count (D187); the
  two load-gated flood benches of local-under-load.txt are a delta applied
  only on the refusing bench's own figure (D196).
- **CI run:** none yet -- this push starts it, so its id goes in the
  merger's report and the next landing's board edit records it. This landing
  discharges that duty for batch 7: run 40937644429, green on 5d422bf in
  both the check and docs jobs, read in full.

## What changed and why

**M13 is half delivered and three expectations move with it.** **E-35**:
the fleet's state is now `status.json` in the state root, published on
change and never on a clock, with `herd-hub status` as the one text screen
over it — a document and not the protocol frame `plan.md` reasoned for
(D215). **E-37**: four of its five events are emitted to one configured
sink, a URL or a command, with no filter and no template (D216); the list
itself was nowhere in this repository until now. **E-41**: its predicate
ships on the requester's side as `herdctl if-moved`, under a lock, refusing
rather than guessing when its state file is unreadable (D217).

**And the hub stops losing a state root to one bad file.** A `job.json` it
cannot read is quarantined — named with its path, counted, out of the queue
— instead of failing `JobDirStore::open` seventy-six lines before the hub
binds. That defect was live on the first consumer's machine and
unrepairable from outside their container (D218, D52, D161).

What `main` can do now that it could not before: **say what the fleet is
doing without a connection**, **tell somebody when a runner joins or is
lost**, **submit a nightly only when a ref has moved**, **open a state root
with a torn record in it**, **tell record bytes from output bytes in the
retention preview**, **name the jobs behind a collection gap**, **name what
a job left behind that it could not reclaim**, **say why a spool was
refused, with the path and the errno**, and **refuse to clear a suite run
that the cgroup cannot carry**.

## Branches in this batch

In the order they were merged:

1. `preflight-cgroup` — ORCH-177 —
   `history/2026-10-29-preflight-cgroup.md`
2. `orch170-policy` — ORCH-170 —
   `history/2026-10-29-orch-170-disk-policy-space-first.md`
3. `v5-operating` — ORCH-178 —
   `history/2026-10-29-v5-operating.md`
4. `claude-md-direction` — ORCH-191 —
   `history/2026-10-29-claude-md-direction.md`
5. `v9-formats` — ORCH-179 —
   `history/2026-10-29-v9-formats.md`
6. `v18-status` — ORCH-180 —
   `history/2026-10-29-v18-status.md`
7. `v20-events` — ORCH-181 —
   `history/2026-10-29-v20-events.md`
8. `v22-schedule` — ORCH-182 —
   `history/2026-10-29-v22-schedule.md`
9. `v10-quarantine` — ORCH-183 —
   `history/2026-10-29-v10-quarantine.md`
10. `orch168-split` — ORCH-168 —
   `history/2026-10-29-orch168-split.md`
11. `orch166-report` — ORCH-166 —
   `history/2026-10-29-orch-166-report-names-short-jobs.md`
12. `v23-janitor` — ORCH-184 —
   `history/2026-10-29-v23-janitor.md`
13. `sweep-names-why` — ORCH-185 —
   `history/2026-10-29-sweep-names-why.md`

## Evidence

**Each branch's own output, separately, never one blurred summary.** Each
branch's handoff above carries its own gate verdict and its own test
counts, and they are **not** restated here as if the merger had re-run
them. What the merger ran is named below, with the sha it ran on, and the
two branches whose own gate was red are named as red.

- `preflight-cgroup` — own gate green. 16 new cases; `check-orchestration`
  269 to 285, re-measured here at **285 passed, 0 failed**.
- `orch170-policy` — docs only, own `check-docs.sh` green. 958 lines in
  `disk-policy.md`, plus `plan.md` and `coverage.md`.
- `v5-operating` — docs only, own `check-docs.sh` green. 805 new lines.
- `claude-md-direction` — `CLAUDE.md` only, own `check-docs.sh` green;
  150 of 150 lines before and after the merge.
- `v9-formats` — own gate green. 8 golden-text tests in
  `herd-proto/tests/frozen_formats.rs` plus `herdctl/tests/frozen_verdict.rs`;
  one-character mutations proved red for all three formats.
- `v18-status` — own gate green. `herd-hub/tests/status.rs`, 552 lines,
  end to end against a real hub and a real runner.
- `v20-events` — own gate green. `herd-hub/tests/events.rs`, 344 lines.
- `v22-schedule` — own gate green. `herdctl/tests/if_moved.rs`, 522 lines.
- `v10-quarantine` — own gate green. The 308-state crash-point census goes
  from `176 refuse the whole state root` to `0`, plus 95 lines in
  `durability.rs`.
- `orch168-split` — own gate green. 583 lines in `retention_preview.rs`.
- `orch166-report` — **own gate RED, four runs, and red on the host and not
  on its code**: one green on an earlier sha, two red with every failure a
  `/chatons.slice` memory refusal, the last red on a quiet host for the same
  cgroup reason. `report.rs`'s own 7 tests passed inside the red runs. Its
  verdict in this landing is the batch gate's and nothing else's.
- `v23-janitor` — **own gate RED, same cause.** Its author re-ran all five
  failing targets with headroom and got 44 assertions, 0 failures. Its
  verdict in this landing is the batch gate's and nothing else's.
- `sweep-names-why` — own gate **GREEN on `5d422bf` exactly**, its base,
  with `check-orchestration` 269 and 0 failed.

**The merger's own gate** runs on this handoff commit, bare, after this
file is committed, and its verbatim output, verdict and receipt path are in
the merger's report — they cannot be in the tree the gate reads.

**The skips, as a list and not a count** (D27, D179, invariant 5): the
twelve at-rest assertion names of `scripts/expected-skips/local.txt`, each
printed as a skip and none counted into a pass. Every one is the same cgroup
v2 delegation this host does not grant:

```
HERD-SKIPPED: a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device
HERD-SKIPPED: a_declared_ceiling_changes_what_the_inert_weight_line_says
HERD-SKIPPED: a_job_cannot_fork_past_its_own_pid_ceiling
HERD-SKIPPED: a_job_s_cgroup_carries_the_cpu_io_and_pid_bounds
HERD-SKIPPED: a_job_s_cgroup_carries_the_hard_io_ceiling_it_was_given
HERD-SKIPPED: a_job_s_record_carries_the_pressure_it_ran_under
HERD-SKIPPED: an_allocation_past_the_ceiling_kills_the_job_and_only_the_job
HERD-SKIPPED: an_io_weight_that_cannot_bite_is_said_out_loud
HERD-SKIPPED: a_tighter_enclosing_memory_max_lowers_the_room_that_is_announced
HERD-SKIPPED: no_declared_ceiling_leaves_io_max_empty
HERD-SKIPPED: no_declared_quota_leaves_cpu_max_at_the_kernel_s_max
HERD-SKIPPED: the_runner_says_which_memory_ceiling_it_enforces
```

The two load-gated flood benches, `flood-fifty` and `flood-two-fifty`, are
a delta applied only on the refusing bench's own figure (D196) and are not
part of the at-rest list. The gate's actual list is in the merger's report
and is what decides; this is what was expected of it.

## Left out

- **`orch162-notes`** — red and never pushed, by its author's correct
  reading of the rule. It would have added 6 `check-orchestration` cases.
  It goes in a later batch; recorded as the second sighting on ORCH-162.
- **The `docs/coverage.md` E-35 and E-40 edits** and **`plan.md`'s M13
  state** — drafted in `v18-status`'s handoff, in nobody's diff, and not
  authored by the merger. ORCH-180.
- **`docs/operating.md`'s placeholder section** — stale in the other
  direction now that M13's half has landed. ORCH-178.
- **No tag**, no release, no release notes: none is owed by this batch and
  a release's own notes are Alexa's own act (D210).
- **No pull request.**
- **The consumer's 37.6 GiB**, deliberately, on Alexa's choice. ORCH-192.
- **The byte-unit fix** — diagnosed here, scoped, and left to its own task
  with its own gate run and golden-text assertions. ORCH-186.

## Decisions allocated

Six, in merge order, with the number allocated to each. `herd-merger`
allocates; numbers are identifiers and gaps stay (D148, invariant 8).

- **D213** — Space before age: the spans become a maximum —
  `orch170-policy`. **Narrowed from its proposal**: deduplication by
  default is a question to Alexa, not a record of his word, and goes to him
  as ORCH-199 together with the removal delay and the archive path.
- **D214** — Alexa directs; the orchestrator keeps work in flight —
  `claude-md-direction`.
- **D215** — The fleet's state is a document, not a frame — `v18-status`.
- **D216** — The events sink: payload, transport, owner's side —
  `v20-events`.
- **D217** — E-41's no-submission skip is the requester's line —
  `v22-schedule`.
- **D218** — An unreadable record is quarantined, not a refused root —
  `v10-quarantine`.

**Refused, with the reason:** `preflight-cgroup`'s 8 GiB cgroup floor, on
its author's own recommendation and the merger's agreement — a threshold in
a script is not a promise to the consumer, and *an unreadable cgroup clears
and says so* is D27 already. It is ORCH-177 instead.

Gaps that stay: **D143**, **D154**, **D170**. D170 is also ORCH-201,
because `check-docs.sh` does not know about it and says so on every run.

## Board rows

**New: ORCH-177 to ORCH-201**, plus **ORCH-168** and **ORCH-170**, the two
ids `docs/disk-policy.md` had already reserved for work in this batch.
ORCH-169 and ORCH-171 to ORCH-175 stay reserved and unused.

`done` — ORCH-168, ORCH-170, ORCH-177 to ORCH-185, ORCH-191, and
**ORCH-166**, which moves from `open`.

`owed` — ORCH-186 (the byte-unit defect), ORCH-192 and ORCH-193 (the spool
re-offer), ORCH-197 (the fleet-wide false silence), ORCH-198
(`herd-runner --help`'s 32 MiB against a 1 GiB constant), ORCH-199 (the two
disk-policy figures for Alexa).

`open` — ORCH-187 (the reaped list), ORCH-188 (Q7 over-promises), ORCH-189
(the quantified roadmap), ORCH-190 (the first duty has no threshold),
ORCH-195 (the runner has no disk figure), ORCH-196 (a runner with no name
changes identity every restart), ORCH-200 (the code had the answer and
threw it away), ORCH-201 (`check-docs.sh`'s stale gap list).

`blocked` — ORCH-194, behind D199's version bump and therefore behind
v1.0.0.

**Updated and not closed:** **ORCH-162** carries its second sighting —
v0.4.1 published without its operator caution too, the second release in a
row, with the prepared half in the tree the whole time. **ORCH-164** says
what a landing cannot close: the code half shipped in v0.4.1 and the runner
can leave 0.3.6, but the row stays open on Alexa's paste, which is his own
act (D210). **ORCH-167** is re-aimed rather than answered by ORCH-192, and
keeps its original question for whatever a re-offer does not clear.

## CI run

**Recorded, not waited on.** This push starts the run, so its id cannot be
in this message; it is in the merger's report, read from
`gh run list --workflow ci.yml --branch main --limit 1` right after the
push, and the **next** landing's board edit is what records it — this
handoff is already inside the commit and a second push for one batch is
refused.

**The landing was not judged risky, and the judgement is argued rather than
asserted:** the gate is green on the gated tree, which is the whole of the
evidence the method asks for; the only two branches whose own gate was red
were red on a host cause this batch's first merge fixes; and the thirteen
branches touch four files in common, every one of which was read line by
line in both directions above. The size of the batch is not itself a risk
when the gate runs on the merged tree.

Batch 7's run, which this landing discharges: **40937644429**, green on
`5d422bf`, both the `check` and `docs` jobs read to completion.

## Processes

PIDs started: `nix develop`, `cargo` and `rustc` children of this task's
own foreground `bash` calls, plus whatever `./scripts/gate.sh` starts after
this commit. Each exited on its own. **Nothing was killed** — no `pkill -f`,
no `killall`, no kill by name — and **PID 3046930, the stray `herd-hub` on
`127.0.0.1:18461`, was left exactly as it is** and is named under *The
host* as worth reclaiming.

`CARGO_TARGET_DIR` is
`…/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/merger-b8-target`,
outside the worktree and off the root filesystem, deleted once the push is
done; the gate's receipts under `wt-batch-8/target/herd-gate/` are kept,
because they are this landing's evidence.

Nothing outside this worktree and this session's scratchpad was written,
with the single exception of step 10's `--ff-only` on the orchestrator's
checkout at `/mnt/ssd_980/herd-project`, which happens after the push and
only if its two preconditions hold. Scratchpad files were given
`merger-b8-` names because the session scratchpad root is shared and two
agents overwrote each other's drafts there today. No container, service,
`/etc/nixos`, `/srv` or anything of the chatOnS project was touched.

## Resume point

`main` carries batch 8. For whoever continues, in order:

1. **Read the merger's report** for the gate's verbatim output, the skip
   list, the merge sha, the gated sha and the post-push run id.
2. **Read that run's verdict** (`gh run view`). A `failure` becomes a board
   row before new work is briefed.
3. **ORCH-199 is the next thing to put to Alexa**, and it is one question:
   §6 E, deduplication on by default, with 5.04 against 4 and the
   day-one-zero cost in the same breath. **ORCH-171 waits on it.**
4. **ORCH-192 is the largest open thing in the product** and needs nothing
   from Alexa. Its reproduction is being written.
5. `orch162-notes` wants re-gating on a quiet host, then a batch.
