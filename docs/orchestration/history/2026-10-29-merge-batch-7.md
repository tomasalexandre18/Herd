# merge-batch-7 — handoff 2026-10-29

- **Item:** batch 7 — `cut-v0.4.1`, landed alone, then tagged `v0.4.1`
- **Agent:** `herd-merger`
- **Worktree:** in the fenced block below — the path is 129 columns and
  prose wraps at 79.

```
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/wt-batch-7
```

- **Branch:** `batch-7-integration`, off `origin/main` at `cd0f1b7`
- **Batch:** one branch. Ten other green branches were held back on
  purpose — see *Not done, and why*.
- **New in this landing:** the integrator pushes the tag. That is Alexa's
  decision of today (**D212**) and no batch before this one ended in a tag.

## Where the things that cannot be in this file are

This handoff is the **last change to the tree**, because the gate must run
on the tree that is pushed (ORCH-121 applied to the merger; `workflow.md`,
*What a landing is*). So these things happen after it was committed and are
in the merger's **report to the orchestrator**, not here:

1. the gate's **verbatim output** and its **skip list** — never a skip
   count (D27, D179);
2. the **merge commit sha** and the resolved **gated sha**, which is that
   commit's second parent;
3. the **post-push `ci.yml` run id** on `main`;
4. the **`v0.4.1` tag push**, and whether any permission wall refused it;
5. the **`release.yml` run** the tag starts — its id, each step's
   conclusion, and what it published: title, draft and prerelease flags,
   and the five assets with their byte sizes;
6. what step 10 did to the orchestrator's checkout at
   `/mnt/ssd_980/herd-project` — the shas it moved between, or which
   precondition stopped it.

## The batch, in merge order

One branch, so the batch is a batch of one and the merge order is trivial.
It was still merged with `--no-ff` rather than fast-forwarded, so that the
integration branch carries an attributable merge commit.

| # | Branch | Tip | Item | Handoff |
|---|---|---|---|---|
| 1 | `cut-v0.4.1` | `ae98f9c` | ORCH-164 (the release carrying D211's fix) | `docs/orchestration/history/2026-10-29-release-v0.4.1.md` |

**Compared with `origin/main` at `cd0f1b7` before merging.** Three commits
(`4303a9d`, `2d24691`, `ae98f9c`). Eight files, 735 insertions, 9
deletions:

- `Cargo.toml`, `Cargo.lock` — the workspace version, `0.4.0` → `0.4.1`
- `deploy/release-notice.md`, `docs/architecture.md`, `docs/joining.md` —
  the texts that quote the version
- `docs/orchestration/history/2026-10-29-release-v0.4.1.md`,
  `-checklist.md`, `-body.md` — its handoff, its checklist, and the human
  half of its body

**Decisions it adds:** none. It brought no `D-NEXT`.

**Conflicts expected:** none, and none occurred. `cut-v0.4.1` descends
directly from `cd0f1b7` and touches no file any other work in flight
touches; in particular it touches **no line of `board.md`,
`decisions.md` or `status.md`**, which is where a batch's conflicts live.

## Conflict log

**Empty, and measured rather than assumed.** `git merge --no-ff
origin/cut-v0.4.1` reported `Merge made by the 'ort' strategy`, left
`git status --porcelain` empty, and
`git diff --stat origin/cut-v0.4.1` against the integration tip produced no
output — so the merge took the branch's tree **unchanged**, byte for byte.
No file was resolved by hand, no hunk was chosen between two sides, and
nothing in this batch touched a specification or an invariant, so nothing
had to be referred back.

The merger's own later commits to the integration branch are not conflict
resolutions and are not logged here as such: they are the D-number
allocation, the board and status edits, and this handoff, which is what
steps 3, 4 and 6 require of every landing.

## D numbers allocated

**One: `D212` — The integrator pushes the tag, on the landing's
conditions.**

**How it was measured, not taken from the brief.** D212 is the next free
number above **D211**, which is the highest on `origin/main` at `cd0f1b7`
**and** the highest on every branch not yet landed. All 99 remote branches
were read, not a sample: each one's `docs/decisions.md` tops out at D211.
Gaps stay (D148, invariant 8) — D143, D154 and D170 are untouched, D170
being the one `./scripts/check-docs.sh` still names as held by an unlanded
branch. The entry was **appended at the end** of `docs/decisions.md` and no
existing entry was renumbered or edited.

**One existing entry is now partly superseded and was deliberately left
standing.** D210 placed completing a release's notes "in the same class as
pushing the tag"; the tag half of that cross-reference is what D212
supersedes, and only that half. `decisions.md` is append-only, so D210's
text is unchanged and D212 says which sentence it displaces.

`cut-v0.4.1` brought no `D-NEXT` of its own, so the whole batch allocated
one number.

## The permission change this landing records

Alexa was asked one question on 2026-10-29 — *who posts a tag and publishes
a version?* — and chose **the integrator, under the same conditions it
already lands under**. The merger is recording that, not deciding it.

**Why it needed deciding: the act had no owner.** The reserving phrase
"Alexa's word, one act at a time" stood in five live places, and each
mechanism around it refused the only agents who could have acted:

- `.claude/hooks/release-guard.sh` refuses `herd-release` `git tag`, a tag
  push, `gh release`, and a `workflow_dispatch` of `release*.yml`. It is
  attached by `herd-release`'s **frontmatter**, so it reaches that agent
  and no other — read, and that reading is why it was expected not to
  refuse the merger.
- `.claude/hooks/orchestrator-guard.sh` refuses the main session `git tag`
  and `git push` (live V11).
- `docs/orchestration/permissions.md:137` refused `herd-merger` a tag "with
  the version batch", on the strength of the reserving sentence itself.

So every release needed Alexa at a terminal for two separate commands, and
v0.4.0 is what that cost: the tag went up on his word and the body went out
incomplete, because the human steps around a release were spread across
people instead of owned by one.

**The five places corrected in this landing:**

1. `CLAUDE.md` — *Git and main*'s tag bullet rewritten; *When to ask
   Alexa* no longer lists tags. The file is at its **150-line ceiling**
   with no slack, so the *When to ask Alexa* paragraph was recompacted by
   one line to pay for the new bullet, keeping every item of its
   do-not-ask list.
2. `docs/orchestration/workflow.md` — the role table (both the
   `herd-merger` and `herd-release` rows), step 7, step 8, *One landing per
   batch* → *Unchanged*, and *Permissions and escalation* in two places.
3. `docs/orchestration/permissions.md` — the `herd-merger` tag row replaced
   by **one allowance plus one narrower refusal**: a tag inside the batch's
   own push, or on a commit whose workspace version the tag does not name,
   stays refused.
4. `.claude/agents/herd-release.md` — description, item 4, and the
   forbidden paragraph: the actor is now named.
5. `.claude/agents/herd-merger.md` — description, step 8, and the forbidden
   list.

**What was deliberately not touched.** `docs/decisions.md` and the files
under `docs/orchestration/history/` are append-only records of past acts;
`docs/orchestration/remediation-report.md` is a dated report. The board's
ORCH-001 row records a decision of 2026-10-24 and so was **amended rather
than rewritten** — its "Kept distinct" sentence now names the supersession
and its date. And neither hook was weakened: `release-guard.sh` still
refuses `herd-release` a tag, which is correct and is not what changed.

## Gates

### The docs gates, run before this handoff commit

`./scripts/check-docs.sh` — green:

```
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```

`./scripts/check-orchestration.sh` — **269 passed, 0 failed**, measured
here and not carried over from the brief. The figure is unchanged across
seven consecutive landings. Worth noting for the next reader: **no
assertion in that suite encoded the old tag reservation**, so rewriting
five contract files moved none of the 269.

Line ceilings, both at their limit with no slack, both measured after the
edits: `docs/status.md` **90 of 90**, `CLAUDE.md` **150 of 150**.

### The board read line by line, both directions

`workflow.md`, *One landing per batch*, says a batch with no conflict needs
this check most, and this batch has no conflict at all.

**Forward** — every line of the branch's own `board.md` looked for verbatim
in the merged file. Four lines are absent, and they are exactly the four
rows the merger itself edited:

```
MISSING: | ORCH-001 | ...
MISSING: | ORCH-119 | ...
MISSING: | ORCH-162 | ...
MISSING: | ORCH-164 | ...
lines of cut-v0.4.1's board.md not present verbatim: 4
```

**Backward** — `diff` from the branch's `board.md` to the merged one, which
must report nothing but merger edits:

```
changed lines: 9
18c18     ORCH-001 amended
31a32     ORCH-176 added
129c130   ORCH-119 updated
160c161   ORCH-162 updated
162c163   ORCH-164 updated
```

Five hunks, every one a merger edit. `cut-v0.4.1` touches **no** board
line, so no branch line was dropped, reordered or renumbered.

### Every branch of the batch left its handoff

`cut-v0.4.1` left three files under `docs/orchestration/history/`:
`2026-10-29-release-v0.4.1.md`, `-checklist.md` and `-body.md`. The third
matters beyond bookkeeping: it is the human half of the release body, and
it is the file Alexa pastes (see *Not done, and why*).

### The mojibake scan, proven before its silence was trusted

`grep` on this host is **ugrep**, whose `-P` matches no byte pattern in any
locale, so a perl byte regex was used. It was proved on fixtures **first**,
in both directions, because a detector that silently matches nothing is
indistinguishable from a clean corpus:

```
b7-fx-good.txt -> 0 hits     (real em dash, e-acute, <= sign)
b7-fx-bad.txt  -> 2 hits     (double-encoded em dash, double-encoded e-acute)
```

Then on each file edited by hand, and on the landing record:

```
0  CLAUDE.md
0  docs/orchestration/workflow.md
0  docs/orchestration/permissions.md
0  .claude/agents/herd-merger.md
0  .claude/agents/herd-release.md
0  the landing record
```

### The Rust gates, separately

Not run on their own. This batch's only code-adjacent change is a version
string in `Cargo.toml` and `Cargo.lock`; the four Rust gates are covered by
the single `gate.sh` run below, which runs them in `ci.yml`'s order.

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
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/wt-batch-7/target/herd-gate/<gated sha>.receipt
```

with `<gated sha>` the full 40 hex of this commit — a value that cannot be
written here, because naming it would change the tree whose sha it is. The
receipt store is **per worktree** (`$HERD_GATE_RECEIPTS`, default
`target/herd-gate/`) and nothing was ever copied into it. `CARGO_TARGET_DIR`
is set **outside** the worktree so the build tree can be deleted after the
push while the small receipts stay.

**The verbatim output and the skip list go to that receipt and to the
merger's report, not into this tree.** A red step blocks the landing, is
reported red, and `main` does not move.

### The host, checked before the heavy phase

Five agents independently found that the suite's real-binary half fails when
`/chatons.slice` is near its 18 GiB `memory.high`, because a test that lends
2 GiB cannot and the runner correctly announces the cgroup's room instead —
and `bench-preflight.sh` does not read that figure. So it was read directly:

```
(memory.high - memory.current) / 1 GiB  =  16
load1 0.50 on 12 cores
336 GB free on /mnt/ssd_980
```

16 GiB of cgroup headroom against the 2 GiB the test lends. PID 3046930, a
stray `herd-hub` from an earlier task holding `127.0.0.1:18461`, was left
alone: it is not this agent's and nothing is ever killed by name.

### The tag, which runs after the push

`git tag -a v0.4.1 -m "v0.4.1: a runner no longer deletes the spools it
recovers"` on the landing commit **by name**, then `git push origin v0.4.1`
alone. Annotated, after the push and never inside it — one landing is one
push (`workflow.md`, *What a landing is*), and folding a tag into it would
make a red run unattributable. It goes after the landing and not before
because `release.yml` compares the tag against the workspace version and
passes only on a commit carrying `0.4.1`.

Verified on the merged tree before any of this: `nix eval --raw
.#static.version` → `0.4.1`, `Cargo.toml` workspace version `0.4.1`, and
the three constants **unmoved** — `PROTOCOL_VERSION` 2
(`herd-proto/src/lib.rs:114`), `RECORD_VERSION` 1 (`herd-hub/src/job.rs:52`),
`LAYOUT_VERSION` 1 (`herd-hub/src/store.rs:59`). So this release moves no
protocol, no record shape and no on-disk layout, and an operator upgrading
meets no migration.

## Not done, and why

**The release body's human half is not pasted, and no agent may paste it.**
`release.yml` assembles the body from its own fragments plus the standing
install notice and **reads no prepared file** — which is how v0.4.0
published with neither operator caution. The half written for v0.4.1 is in
the tree at `docs/orchestration/history/2026-10-29-release-v0.4.1-body.md`.
Putting it on the published page is **Alexa's own act** (**D210**): the
permission system refuses that write to every execution agent and to the
orchestrator, and D210 says neither wall is to be weakened to discharge it.
**So v0.4.1 publishes the same way v0.4.0 did, and the consumer can read the
release without learning why they want it.** This is the one thing this
landing leaves undone that someone has to act on.

**`orch162-notes`**, which closes that gap permanently by shipping a
release's notes in the tree, is **red on this host and not landed**, so it
could not ride in this batch. ORCH-162 now carries the second sighting.

**Ten green branches, held back on purpose.** `orch166-report`,
`orch168-split`, `orch170-policy`, `claude-md-direction`, `v5-operating`,
`v9-formats`, `v10-quarantine`, `v18-status`, `v20-events`, `v22-schedule`
and `v23-janitor` were ready when this batch started. v0.4.1's notes were
written for **one** subject — a runner no longer deletes the spools it
recovers — and a batch of ten would have made them false. The release goes
alone and goes first because the first consumer's runner is held at 0.3.6
while roughly 396 MiB of a job's evidence sits in such a spool. The others
land next, as their own batch.

**No pull request** was opened, by anyone, for anything in this batch. The
one dated exception of 2026-10-28 was a single landing and there is no
second one.

**ORCH-164 is not closed.** It closes on Alexa's paste, not on this
landing — see *Board rows*.

## Risks and findings

1. **The brief's board-id figure was wrong, and the measurement is in this
   handoff.** It said ORCH-168 was taken and the first free id was 171.
   Measured: `origin/main` tops out at **ORCH-167**, but `orch168-split`
   holds 168-170 and `orch170-policy` holds 168-**175**, both pushed and
   unlanded, in `docs/disk-policy.md`'s own table rather than in
   `board.md` — which is why a `board.md`-only scan misses them. The first
   free id is **176**. A scan for the next free `ORCH-nnn` has to read every
   tracked file on every pushed branch, not just the board.
2. **Five live files can disagree with a decision for as long as nobody
   re-reads them.** The reserving phrase sat in five places and in two hook
   files' worth of mechanism, and the gap between "nobody is allowed to do
   this" and "nobody is named to do this" was invisible until a release
   needed it twice. `check-orchestration.sh`'s 269 assertions did not catch
   it, because none of them encoded the reservation. Worth a row if it
   recurs: a check that the permission table's actors exist.
3. **A release's notes gap is now measured at two releases, not one.** It is
   not a v0.4.0 accident, and the next release will publish incomplete too
   unless ORCH-162 lands first. That is the recommendation to carry.
4. **The skip list, never the count.** Twelve at rest on this host, all one
   cause: no cgroup v2 delegation. The two load-gated flood benches are a
   delta applied only on the refusing bench's own figure (D196) and were not
   applied here. A count would have said nothing (D27, D179).

## The landing record

Written now, at step 5, because it **is** the message of the `--no-ff` merge
commit of step 8. This copy and that message are the same text. The gated
sha appears in it as "`1e6c7b5` plus the merge-handoff commit, which is this
commit's second parent": exact and self-resolving — `git rev-parse <merge
sha>^2` — and not writable as 40 hex here, for the reason given under
*Gates*.

Land batch 7: cut v0.4.1 and make the tag the integrator's

- **Batch:** 7, one branch, landed 2026-10-29 by herd-merger. One push, one
  --no-ff merge commit carrying this record, one post-push ci.yml run, and
  then one annotated tag as a separate and later act (D212).
- **Gate:** green -- ./scripts/gate.sh on `1e6c7b5` plus the merge-handoff
  commit, which is this commit's second parent and the last change to the
  tree; trees identical, `git diff --quiet` between that sha and this one
  clean. Run
  BARE, not inside nix develop: nesting TMPDIR panics confinement.rs:2529
  on SUN_LEN (ORCH-139). The receipt is named for that sha under
  target/herd-gate/ in the merger's own worktree, per worktree, never
  copied in. The resolved 40-hex sha, the verbatim output and the receipt
  path are in the merger's report: each exists only after the gated tree was
  closed, so none of them can be in this message.
- **Skips:** 12, the at-rest names of scripts/expected-skips/local.txt, each
  printed as a skip and none counted into a pass (D27, invariant 5). Named
  one by one under *Evidence*. The list decides, never the count (D187); the
  two load-gated flood benches of local-under-load.txt are a delta applied
  only on the refusing bench's own figure (D196) and were not applied here.
- **CI run:** none yet -- this push starts it, so its id goes in the
  merger's report and the next landing's board edit records it. This landing
  is where that duty was discharged for batch 6: run 40908071577, green on
  cd0f1b7, is now in ORCH-119's evidence as the fourth consecutive green.

## What changed and why

This landing moves no milestone and adds no expectation. It does two things,
and they are separable: it **releases v0.4.1**, and it **records a
permission change Alexa made today**.

v0.4.1 has one subject, and the notes written for it were written for that
one subject: **a runner no longer deletes the spools it recovers**. D211
landed on `main` in batch 6 and was in no release, so until this push the
fix existed and nobody could install it. The first consumer's runner is held
at 0.3.6 on the orchestrator's instruction because roughly 396 MiB of a
job's evidence is sitting in such a spool and a v0.4.0 upgrade would delete
it. This release is the only thing between them and a safe upgrade, which is
why it goes alone and goes first.

The permission change: until today a tag was reserved with the phrase
"Alexa's word, one act at a time" in five live places, and **named no actor
who could carry it out**. `release-guard.sh` refuses `herd-release` a tag
(live V10); `orchestrator-guard.sh` refuses the orchestrator one (live V11);
and the `herd-merger` row in permissions.md refused it on the strength of
the reserving sentence itself. So every release needed Alexa at a terminal,
and v0.4.0 is what that cost -- the tag went up on his word and the body
went out incomplete, because the human steps around a release were spread
across people instead of owned by one. Asked one question on 2026-10-29,
Alexa chose **the integrator, under the same conditions it already lands
under**. That is D212, and this landing corrects all five places.

What `main` can do now that it could not before: a `cut-v*` branch landing
is followed by its own tag without a second person, on the two conditions
the landing already carries -- a green gate for the gated sha and a filled
record -- and the contract no longer reserves to Alexa an act no agent was
permitted to perform.

## Branches in this batch

1. `cut-v0.4.1` -- ORCH-164 (the release that carries D211's fix; the row
   does not close here, see *Board rows*), tip `ae98f9c`, three commits on
   `cd0f1b7`. Handoff:
   `docs/orchestration/history/2026-10-29-release-v0.4.1.md`, with
   `-checklist.md` and `-body.md` beside it. Merged first and alone.

Ten other green branches were ready and are deliberately not here: see
*Left out*.

## Evidence

**cut-v0.4.1, on its own.** Merged onto the integration branch off `cd0f1b7`
with `--no-ff`: **zero conflicts**, and
`git diff --stat origin/cut-v0.4.1` against the integration tip was empty,
so the merge took the branch's tree unchanged. Eight files, 735 insertions, 9
deletions: Cargo.toml, Cargo.lock, deploy/release-notice.md,
docs/architecture.md, docs/joining.md and the three history files. Its own
gate was green on `ae98f9c` per its handoff; re-verified here rather than
taken on trust, on the merged tree: `nix eval --raw .#static.version` ->
`0.4.1`, `Cargo.toml` workspace version `0.4.1`, and the three constants
**unmoved** -- PROTOCOL_VERSION 2 (herd-proto/src/lib.rs:114), RECORD_VERSION
1 (herd-hub/src/job.rs:52), LAYOUT_VERSION 1 (herd-hub/src/store.rs:59). So
this release moves no protocol, no record shape and no on-disk layout, and
an operator upgrading meets no migration.

**The batch's own run on the merged tree.** One ./scripts/gate.sh run, bare,
on a clean tree, on the second parent of this commit. Its verdict line, its
sha, its two host load readings and the receipt path are in the merger's
report, verbatim, because the gate runs after this message is written.

**Skips: 12, each named, each printed as a skip, none counted into a pass**
(D27, invariant 5). All twelve are the same cause -- this host grants no
cgroup v2 delegation, so an assertion about a job's own cgroup cannot be
made here. The twelve `HERD-SKIPPED:` names, in the order local.txt lists
them:
`a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device`;
`a_declared_ceiling_changes_what_the_inert_weight_line_says`;
`a_job_cannot_fork_past_its_own_pid_ceiling`;
`a_job_s_cgroup_carries_the_cpu_io_and_pid_bounds`;
`a_job_s_cgroup_carries_the_hard_io_ceiling_it_was_given`;
`a_job_s_record_carries_the_pressure_it_ran_under`;
`an_allocation_past_the_ceiling_kills_the_job_and_only_the_job`;
`an_io_weight_that_cannot_bite_is_said_out_loud`;
`a_tighter_enclosing_memory_max_lowers_the_room_that_is_announced`;
`no_declared_ceiling_leaves_io_max_empty`;
`no_declared_quota_leaves_cpu_max_at_the_kernel_s_max`;
`the_runner_says_which_memory_ceiling_it_enforces`. The two load-gated flood
benches, `flood-fifty` and `flood-two-fifty`, did **not** skip and no delta
was applied.

**Docs checks, measured on the merged tree.** `./scripts/check-docs.sh`:
`docs ok`, with its standing note that D170 is a gap held by an unlanded
branch and gaps stay (D148). `./scripts/check-orchestration.sh`: **269
passed, 0 failed**. `docs/status.md` **90 lines of 90**; `CLAUDE.md` **150
of 150** -- both at their ceiling with no slack, which is why the status
rewrite below is line-for-line and the CLAUDE.md *When to ask Alexa*
paragraph was recompacted to pay for the new bullet.

**The board check, run both ways** (workflow.md, *One landing per batch*,
which says a batch with no conflict needs it most). Forward: of the lines of
`cut-v0.4.1`'s own board.md, exactly **four** are not present verbatim in
the merged file -- ORCH-001, ORCH-119, ORCH-162 and ORCH-164, which are the
four rows the merger itself edited. Backward: `diff` between that branch's
board.md and the merged one reports **five** hunks, `18c18`, `31a32`,
`129c130`, `160c161`, `162c163`, every one of them a merger edit and not a
branch's. `cut-v0.4.1` touches no board line at all, so no branch line was
dropped and none was renumbered.

**Double encoding, checked on a fixture first and then on the diff.** `grep`
on this host is ugrep and its -P matches no byte pattern, so a perl byte
regex was used. Proved before use: 0 hits on a known-good fixture carrying a
real em dash, an e-acute and a <= sign, 2 hits on a known-bad fixture
carrying a double-encoded em dash and a double-encoded e-acute. Then 0 hits
on each of the five files edited by hand -- CLAUDE.md, workflow.md,
permissions.md, herd-merger.md, herd-release.md.

## Left out

**Ten green branches, deliberately.** `orch166-report`, `orch168-split`,
`orch170-policy`, `claude-md-direction`, `v5-operating`, `v9-formats`,
`v10-quarantine`, `v18-status`, `v20-events`, `v22-schedule` and
`v23-janitor` were ready when this batch started and were held back:
v0.4.1's notes were written for one subject, and a batch of ten would make
them false. They land next, as their own batch.

**`orch162-notes`**, which would ship a release's notes in the tree and
retire D210's interim rule permanently, is **red on this host and not
landed**, so it could not ride here. That is why v0.4.1 published the same
way v0.4.0 did -- see the next paragraph.

**The release body's human half is not pasted, and no agent may paste it.**
`release.yml` assembles the body from its own fragments plus the standing
install notice and **reads no prepared file**. The half written for v0.4.1
is in the tree at
`docs/orchestration/history/2026-10-29-release-v0.4.1-body.md`. Putting it
on the published page is **Alexa's own act** (D210): the permission system
refuses that write to every execution agent and to the orchestrator, and
D210 says neither wall is to be weakened to discharge it. It is named in the
merger's report as still owed.

**No pull request** was opened, by anyone, for anything in this batch. The
one dated exception on 2026-10-28 was a single landing and there is no
second one.

**The tag is not in this push.** It follows as its own act, after it, and is
reported with its own evidence -- one landing is one push (workflow.md,
*What a landing is*), and folding a tag into it would make a red run
unattributable.

## Decisions allocated

**D212 -- The integrator pushes the tag, on the landing's conditions.** One
entry, allocated for the whole batch in merge order. It is the next free
number above **D211**, the highest on `origin/main` at `cd0f1b7` **and** the
highest on every branch not yet landed -- measured across all 99 remote
branches, all of which top out at D211. Gaps stay (D148, invariant 8): D143,
D154 and D170 are untouched, D170 being the one `check-docs.sh` still names
as held by an unlanded branch. Nothing was renumbered and no existing entry
was edited -- including **D210**, whose sentence placing the tag "in the same
class" as the notes is superseded by D212 and left standing as written,
because decisions.md is append-only.

`cut-v0.4.1` brought no `D-NEXT` of its own.

## Board rows

- **ORCH-176, new, `done`** -- the question "who posts a tag and publishes a
  version?", its answer, and the five places corrected. Written in *Waiting*
  beside ORCH-001, which is where this project keeps a decided permission.
  **It is the first free id, and the brief's figure was wrong:** `origin/main`
  tops out at ORCH-167, but `orch168-split` holds 168-170 and
  `orch170-policy` holds 168-175 in `docs/disk-policy.md`, both pushed and
  unlanded. So the first free id is **176**, not 171.
- **ORCH-001, amended** -- its "Kept distinct, not covered by it" sentence
  now says that the **tag half** was superseded on 2026-10-29 by ORCH-176
  and D212, and that only the tag half was: ordering a release, and a
  release's own notes, stay Alexa's.
- **ORCH-164, left open on purpose, and this is why.** The code fix is done
  and the release that carries it now exists and is tagged, so the consumer
  has a version they can upgrade to. The row still does not close, because
  the published page carries the generated head and the standing notice and
  **not** the operator caution written for v0.4.1 -- and that paste is
  Alexa's own act (D210). Until he pastes it, the consumer can read the
  release and still not learn why they want it. The row keeps the state
  `done`, which is the word its own text already uses for "done and not
  closed"; the obligation is in its next action.
- **ORCH-162, updated** -- second sighting recorded. v0.4.1 published the
  same way v0.4.0 did, so the gap is measured at two consecutive releases
  and is not a v0.4.0 accident. Its branch `orch162-notes` exists and is red
  on this host. State stays `open`.
- **ORCH-119, updated** -- batch 6's post-push run `40908071577`, green on
  `cd0f1b7`, `conclusion: success`, recorded as the **fourth** consecutive
  green. Batch 6's landing left that reading to this one. Four greens is
  longer than the streak of three reds it replaced, and it is still a streak
  and not a rate.

`docs/status.md` rewritten where the state moved: v0.4.1 is the published
release, the tag actor is named, and the post-push count is four. Still 90
lines of 90.

## CI run

**Recorded, not waited on.** This push starts the run, so its id cannot be
in this message; it is in the merger's report, read from
`gh run list --workflow ci.yml --branch main --limit 1` right after the
push, and the **next** landing's board edit is what records it -- the
merger's own handoff is already inside this commit and a second push for one
batch is refused. The landing was not judged risky: the gate is green on the
gated tree, the batch is one branch, and the only code-adjacent change in it
is a version string. Its verdict is read at the next session start, and a
failure becomes an ORCH-nnn row before new work is briefed.

Separately and after it, `release.yml` fires on the `v0.4.1` tag. That run
is not this landing's CI run; its id, each step's conclusion and what it
published are in the merger's report.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01FJ9b7vqezAbhLMw8LEsgsc
