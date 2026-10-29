# merge-batch-4 — handoff 2026-10-29

- **Item:** batch 4, the fourth local landing: the ordinary release
  `cut-v0.4.0`. **ORCH-156** half discharged and still `owed`; **ORCH-139**
  amended; **ORCH-119** amended with the first green post-push run on `main`;
  **ORCH-161** opened. **No `D` number allocated**, argued below. **No tag,
  no release, no pull request.**
- **Agent:** herd-merger
- **Branch / commit:** integration branch `batch-4-integration`, off
  `origin/main` `6036d24`; this handoff is its tip. Its worktree, created by
  this agent under its own session scratchpad (ORCH-031):

  ```text
  /mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/wt-batch-4
  ```
- **Landing:** this one. **One branch, one `--no-ff` merge commit whose
  message is the record below, one push, one post-push `ci.yml` run on
  `main`.** The record is copied whole under *The landing record* and is that
  commit's message.
- **Where the things that cannot be here are:** the gate's verbatim output,
  its applied skip list, the merge commit sha, the post-push run id, and what
  step 10 did to the orchestrator's checkout are all in **herd-merger's
  report to the orchestrator**, not in this file. They happen after this file
  is committed, and this file is in the tree the gate runs on (ORCH-121).

## Done

### 1. The batch, listed before anything was merged

Fetched, then compared with `origin/main` `6036d24`. The batch is **one
branch** — everything that was ready when this agent started — and a batch of
one gets the same checks as a batch of three, which is the whole point of the
check in section 3.

```text
cut-v0.4.0  1dd2383ba289b8c5937d0a94c53ebcc164fb1212
            one commit on 6036d24, which IS origin/main
            8 files, +734 −9
```

**Files touched, against `origin/main`:**

| branch | files | what | decisions | conflicts expected |
|---|---|---|---|---|
| `cut-v0.4.0` | 8 | `Cargo.toml`, `Cargo.lock` (0.3.7 → 0.4.0 in four crates), `deploy/release-notice.md`, `docs/architecture.md`, `docs/joining.md` (the version string in each), and three new history files: the handoff, the release body, the checklist | **none**; no `D-NEXT` header anywhere | none |

**No `.rs` file, no `Cargo` dependency, nothing under `.github/`, and no file
under `docs/decisions.md`, `docs/orchestration/board.md` or
`docs/status.md`** — so every row and every line of those three in this
landing is `herd-merger`'s own, written from the release agent's handoff and
the orchestrator's brief.

**`D-NEXT`: none.** `git grep D-NEXT` over the branch's whole `docs/` tree
returns only prose about the mechanism — `decisions.md:8197`, `:8200`,
`decisions-structure.md:37`, `:40`, `:47` and four board rows quoting it —
and not one header. Nothing to allocate.

### 2. The conflict log

**Empty, and measured rather than assumed.** The single `--no-ff` merge
reported `Merge made by the 'ort' strategy` with no unmerged path, and
`git status --porcelain` was silent straight afterwards. Nothing was resolved
by hand, so nothing could have changed a specification or an invariant by
resolution and nothing was escalated under step 2.

The merge order was trivial — one branch — but it was still one `--no-ff`
merge of its own, `1dd2383` → `4e04193`, so that a later red run names a
merge commit and not a blur.

### 3. The board check, both ways

Run precisely **because** there was no conflict: a batch git merges without
asking is where a silent revert hides, and nothing but this check would catch
it.

**Direction one, every branch's own `board.md` line verbatim in the merged
file:** `cut-v0.4.0` touched neither `board.md` nor `docs/status.md` —
measured, `git diff --stat origin/main origin/cut-v0.4.0 --` on both paths is
empty — so there was **no branch line to require**. That is a real answer and
not a skipped check, and it is why direction two was run properly.

**Direction two, the merged file differs from each input only by the other
party's hunks.** With one branch, the only other party is the merger, so the
merged board was compared column by column against `origin/main`'s:

```text
rows on origin/main: 139      rows now: 140
rows dropped:        none     (comm -23 over the sorted id lists: empty)
rows changed:        ORCH-119 ORCH-139 ORCH-156
rows added:          ORCH-161 (8 columns, state: open)

  ORCH-119  col 2 grew by  589 chars (append-only)
  ORCH-119  col 8 grew by   95 chars (append-only)
  ORCH-139  col 2 grew by 1040 chars (append-only)
  ORCH-139  col 8 grew by  190 chars (append-only)
  ORCH-156  col 6 grew by 1009 chars (append-only)
  ORCH-156  col 8 grew by  110 chars (append-only)

VERDICT: every pre-existing row keeps all of its old text, column for
column, as a prefix; nothing reverted, nothing dropped, 8 columns
everywhere.
```

The check is a prefix comparison per cell, so a cell that lost a word would
fail it rather than pass quietly. It did fail once, on its own framing bug —
the first version split the row with its `| ` and ` |` still attached, so an
append to the last cell was not a prefix — which is worth recording because a
check that can only pass is not a check.

**Every branch left its handoff** under `docs/orchestration/history/`:
`2026-10-29-release-v0.4.0.md`, with `-body.md` and `-checklist.md` beside it.

### 4. `docs/decisions.md`: nothing allocated, and the argument for it

**No number was allocated and the file was not touched.** The highest entry
on `origin/main` is **D209** (measured here over the `## <date> — Dnnn`
headers, 206 of them), so **D210 remains the next free number**, and D143,
D154 and D170 stay gaps (D148, invariant 8). No branch of this batch holds a
number above D209, there being one branch and it holding none.

Two findings landed in this batch and either could have earned an entry. The
test applied: **a decision entry records a choice that constrains future work
and would otherwise be re-litigated.**

- **ORCH-139's amendment** is a *measurement correcting a wrong diagnosis* in
  a row that already exists and already owns the fix. It constrains nothing,
  and a reader looking for "how do I run the gate" looks at the row or at
  `gate.sh`, never at a decision number.
- **ORCH-161** makes no choice at all. **D199** already governs the version
  ordering and **ORCH-106** already owns the behaviour and its fix; this
  records a consequence of both, in the place an operator reads.
- **The release itself** allocates none, which was `herd-release`'s judgement
  and the orchestrator's; the merger re-read **D199** and agrees — it already
  says the ordinary release ships at layout 1 and record 1.

If the orchestrator disagrees on any of the three, the cost is one entry in a
later batch and nothing here has to be undone. That asymmetry is why the
judgement went this way.

### 5. The three constants, re-read rather than taken on report

The brief had verified these and asked the merger not to redo it. They were
re-read anyway, on the **merged** tree, because they are D199's binding
constraint and a release is where a silent bump would do the most damage:

```text
PROTOCOL_VERSION = 2   herd-proto/src/lib.rs
RECORD_VERSION   = 1   herd-hub/src/job.rs
LAYOUT_VERSION   = 1   herd-hub/src/store.rs
workspace version      0.4.0   Cargo.toml:6
```

**No `v0.4.0` tag exists**, locally (`git tag -l 'v0.4*'` empty) or on the
origin (`git ls-remote --tags origin` has no match). **This landing creates
none**: Alexa has given the word for the release and *not* for the tag, and
the tag is his own separate act against this merge commit.

### 6. The docs work the cut left to the merger

All four were owed and all four are done, in one commit `39d6595`.

**`docs/status.md`** — two false sentences, now true, and the file is still
**90 of its 90 lines**. It said *"`main` is 322 commits past v0.3.7 and the
workspace version still reads `0.3.7`"*. Re-measured here with
`git rev-list --count v0.3.7..origin/main`: **334 commits and 84 merges**, so
the figure was already wrong before the bump. The section now says v0.4.0 is
cut, gated and landed **and not released**, names the tag as Alexa's own act,
states the three version numbers as unmoved, and records the broken red
streak. There was no slack to spend, so three lines were reclaimed: one from
the v1.0.0 section, which restated ORCH-156's constraint that the releases
section now carries, and two from the post-push paragraph, whose three reds
are history now that a green run exists.

**ORCH-156** — half discharged, and deliberately **still `owed`**. The
ordinary release is cut, gated and landed at layout 1 and record 1, which is
the whole of what this row asks *of a release*. It is not `done` because
*shipped* means tagged and published; and ORCH-145's reading, the other half,
is the consumer's to take on their own root, which nobody here may touch
(D161, invariant 9).

**ORCH-139** — amended, in one line as asked. The row's title blames *"a long
`TMPDIR`"* and names only one of two triggers; the second cost the release
agent a RED gate. The rule written into the row: **run `./scripts/gate.sh`
BARE, never under `nix develop --command`**, because `gate.sh` already wraps
each toolchain step in `nix develop` itself (its lines 244, 246, 253), so
wrapping the whole script nests the shells and with them `TMPDIR` — 45
characters at one level, **62** at two — and `TempDir::new`
(`herd-runner/src/testing.rs:141`) adds about 53 bytes, which tips the total
past the 108-byte `SUN_LEN` and panics `confinement.rs:2529`. The release
agent measured the **same sha RED nested and GREEN bare**, with `TMPDIR`
already pointed at the short session directory this row calls good. **This
gate was run bare**, which is section 7.

**ORCH-119** — the fourth post-push run on `main` and the **first green one**,
which batch 3's step 9 could not record itself: **`40870735400` at
`6036d24`, conclusion `success`**, 2026-10-29T12:07:41Z. Read here with
`gh run list --workflow ci.yml --branch main --limit 5` rather than taken
from the brief, which also confirmed the three reds it follows —
`40857453150` (`b73d38c`), `40762764493` (`09229db`), `40720983497`
(`7eb9585`). It is the first second machine to agree with **D208**. **One
green run is one sample and not a rate:** it closes nothing, the bounded race
D208's handoff records is unchanged, and **ORCH-160 stays open**.

### 7. ORCH-161, and why it is a row of its own

The release agent listed this as a finding *for the orchestrator* rather than
as something it had put in the release body, and the body did not carry it.
It does now.

**The finding, re-verified on this tree and not at `2ca6dc7`, whose line
numbers in ORCH-106 have drifted:**

```text
herd-hub/src/store.rs:1339-1340  an unparseable job.json -> Err, via
:1348-1349                       failed("reading", &path, &error)?
:1358-1359
:1351-1354                       ...while a rejected LABEL SET three lines
                                 on returns Ok(Stored::Quarantined(..))
:1097                            recover() propagates it with ?
:481                             open_with() propagates that with ?
herd-hub/src/main.rs:358         main() propagates THAT with ?
herd-hub/src/main.rs:434         ...and Hub::bind_with is 76 lines later
```

So one torn record refuses the **whole state root** and the hub **exits
before binding a port**. **`herd-hub report` is not affected the same way:**
an unparseable record is pushed onto `census.unread` and the loop continues
(`herd-hub/src/report.rs:334-341`), exactly as a `RECORD_VERSION` mismatch is
(`:344-353`) — it counts what it cannot read instead of refusing everything.

**Checked before claiming it to an operator:** `read_stored` is byte-for-byte
the same shape in `v0.3.7` (`store.rs:1136` there), so *"the refusal is in the
build you are running today"* is measured and not inferred. That sentence is
in the release body because without it the body would read as if v0.4.0 had
introduced the problem.

**A row of its own, not an amendment to ORCH-106 — and the reason is
findability, not novelty.** ORCH-106 is a *product* row owned by `herd-step`
whose cell already carries its brief verbatim. This one has a different owner
(whoever writes a release body), a different artefact (operator-facing prose,
no code) and a different lifetime — it closes the day ORCH-106 lands. Folded
into ORCH-106, nobody writing a release body would ever find it.

### 8. The release body's one substantive addition

`docs/orchestration/history/2026-10-29-release-v0.4.0-body.md`, section
*Upgrading and rolling back*, whose last paragraph said *"The one practical
caution is the stop budget"*. There are two now. The second is the order:
**take the `report` reading before starting a v0.4.0 hub on that root**, with
the mechanism in operator's words, the `report` contrast, and three sentences
saying plainly that this is not new in v0.4.0, is not caused by it, is a
defect on our side, and is being fixed. Nothing else in the body was touched.

## Gates

- **Gate:** green — `./scripts/gate.sh` on the tip off `39d6595`, run
  **BARE** per the amended ORCH-139. That tip is **this file's own
  commit**, which this file cannot name, and it is the second parent of
  the merge commit; its receipt is written to `target/herd-gate/` of the
  integration worktree, named for that sha, and the receipt store never
  leaves the worktree. **The verbatim output, the verdict line and the
  applied skip list are in herd-merger's report to the orchestrator** — they
  cannot be in this tree, because the gate ran on this tree (ORCH-121).
- **Run before this file was committed, and quoted here:**
  `./scripts/check-docs.sh` exit **0**, `docs ok`, with its standing note
  that D170 is a gap and gaps stay (D148); and
  `./scripts/check-orchestration.sh` **269 passed, 0 failed** — unmoved from
  `b73d38c`, `6036d24` and `1dd2383`, now a fourth independent measurement.
  Both were run on the baseline merge and again after every docs edit, not
  once at the end.
- **Rust gates: not run separately, by design.** This batch touches no `.rs`
  file and no `Cargo` dependency; `Cargo.toml` and `Cargo.lock` carry the
  version bump only. The four Rust gates are run by `gate.sh` as its own
  steps 0..8 on the merged tree, which is the run that counts.
- **The double-encoding detector**, the perl byte regex for
  `\xc3\xa2\xc2[\x80-\xbf]\xc2[\x80-\xbf]` and `\xc3\x82\xc2[\x80-\xbf]`, was
  run over every tracked `.md` after every edit. **One hit, and it is a false
  positive that proves the detector works:** line 188 of
  `history/2026-10-29-merge-batch-3.md` reproduces the two-byte mangled shape
  on purpose, as the worked example in its own description of this detector.
  It is on `main` already and untouched here. **Nothing this landing wrote is
  double-encoded**, and that sentence is kept literally true: this file
  describes that example rather than reprinting its bytes, so the detector
  stays at exactly one hit and the next reader knows which file it is.
- **No file lost its mode.** Every edit was written in place by a perl script
  using `use utf8` and an encoding layer on the handle, never `awk`+`mv`;
  `git diff --summary` against `origin/main` shows three `create mode 100644`
  from the branch and no mode change.

## The landing record

The merge commit's message, in full. One thing in it cannot be identical to
the commit's own copy, and it is said rather than hidden: the `GATEDSHA`
token below is **this handoff's own commit sha**, which the merge message
carries and which this file cannot contain. Everything else is word for word.

```text
Land batch 4: v0.4.0, read your records before the versions move

- **Batch:** 4, one branch, landed 2026-10-29 by herd-merger. One --no-ff
  merge commit whose message is this record, one push, one post-push
  ci.yml run on main that is then read. A single branch is a batch of one
  and gets the same checks; every figure below was re-measured on this
  tree rather than carried over from the brief.
- **Gate:** green -- ./scripts/gate.sh, run BARE, on GATEDSHA, this
  commit's second parent, with `git diff --quiet GATEDSHA HEAD` silent, so
  the gated tree and the pushed tree are identical. Receipt
  target/herd-gate/GATEDSHA.receipt in the integration worktree, where the
  receipt store lives and which it never leaves. The verbatim output and
  the applied skip list are in herd-merger's report to the orchestrator:
  they cannot be in this tree, because the gate ran on this tree
  (ORCH-121).
- **Skips:** the twelve at-rest names of scripts/expected-skips/local.txt,
  every one of them the same cgroup v2 delegation this host does not
  grant, plus the two load-gated flood benches of local-under-load.txt if
  and only if their own skip lines excuse them by name and by their own
  load figure (D196). Each is printed as a skip and never counted as a
  pass (D27, invariant 5). The list decides and the count does not (D179);
  the twelve are named under *Evidence* and the applied list is in the
  verdict line.
- **CI run:** none yet -- this push starts it. The run cannot exist before
  the commit that triggers it, so its id goes to herd-merger's report now
  and the next landing's board edit records it; a second push for one
  batch is the one thing the method forbids.

## What changed and why

This is the ordinary release that ORCH-156 and D199 require: the last one
before PROTOCOL_VERSION 2 -> 3, RECORD_VERSION 1 -> 2 and LAYOUT_VERSION
1 -> 2, none of which has a migration path and the last two of which
destroy an operator's state root. It moves no milestone and delivers no
new E-nn expectation. It bumps the workspace to 0.4.0 so that a build
exists in which an operator can read the state root they already have,
while that root is still readable at layout 1 and record 1.

What main can do that it could not before: call itself 0.4.0, with all
three version numbers deliberately unmoved -- and verified unmoved on this
tree, read out of the source rather than taken on report: PROTOCOL_VERSION
2 (herd-proto/src/lib.rs), RECORD_VERSION 1 (herd-hub/src/job.rs),
LAYOUT_VERSION 1 (herd-hub/src/store.rs). No .rs file is touched by this
batch at all.

It also lands one operator-facing correction the cut did not carry: the
release body now says to take the `herd-hub report` reading BEFORE
starting a v0.4.0 hub on an old root, because one torn job.json refuses
the whole state root at startup while report counts what it cannot read.
That is ORCH-161, opened here.

## Branches in this batch

1. cut-v0.4.0, 1dd2383ba289b8c5937d0a94c53ebcc164fb1212, ORCH-156 -- the
   row whose ordering constraint this release half discharges. Handoff:
   docs/orchestration/history/2026-10-29-release-v0.4.0.md, with its
   release body (-body.md) and its checklist (-checklist.md) beside it.
   One commit on 6036d24, eight files, no .rs and no .github. Merged
   first and only, --no-ff, giving 4e04193 on batch-4-integration.

A batch of one. Then 39d6595, the merger's own board, status and
release-body edits, and this landing's handoff as the integration tip --
the sha the gate ran on.

## Evidence

**Each branch's own evidence, kept its own and not blurred into one
summary.**

cut-v0.4.0's own gate, GREEN, read from its receipt and not from its
prose: verdict GREEN, sha 1dd2383, steps 0..8 all ok, skips-observed 12,
load-excused-skips none, skip-list-sha256 0fbdadcaf14aac -- the same
skip-list hash as batch 3, which is what says the skip set did not move.
Read by the orchestrator at verification; not re-run here, because a
receipt is per worktree and per sha and the batch's own gate below is the
one that counts.

cut-v0.4.0's interoperability evidence, which is the claim the release
rests on and was tested rather than argued: a state root written by the
v0.3.7 binaries -- store.json = {"layout_version":1,"state_root_id":
"root-b336b9c9..."} with no ids_minted field -- was opened by this
build's `herd-hub report`, which read every record: job directories 4 /
records read 4 / directories with no record yet 0 / records this build
could not read 0, with r_file 1.80 = 9216/5120, which checks by
arithmetic.

The batch's own gate on the merged tree: ./scripts/gate.sh run BARE on
GATEDSHA, the integration tip and this commit's second parent, with
`git diff --quiet GATEDSHA HEAD` silent. Its verbatim output and its
applied skip list are in herd-merger's report; they cannot be in this
tree, which is the tree the gate ran on (ORCH-121).

Re-measured by the merger on the merged tree, each with where it came
from:

- ./scripts/check-docs.sh exit 0, "docs ok", with its standing note that
  D170 is a gap held by a closed PR and gaps stay (D148).
- ./scripts/check-orchestration.sh 269 passed, 0 failed -- unmoved from
  b73d38c, 6036d24 and 1dd2383, now measured a fourth time here.
- 334 commits and 84 merges between v0.3.7 (8422abd) and origin/main,
  by git rev-list --count, which is what corrects status.md's 322.
- No v0.4.0 tag exists, locally or on the origin: git tag -l and
  git ls-remote --tags both empty for it. This landing creates none.
- The post-push run of batch 3: 40870735400 at 6036d24, conclusion
  success, read by `gh run list` here and not taken from the brief.

The twelve at-rest skips, named, each printed as a skip and never as a
pass (D27, invariant 5). All twelve are the same cgroup v2 delegation
this host does not grant:

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

Two more may be excused under load, and only on their own word: the
flood-fifty and flood-two-fifty benches of
scripts/expected-skips/local-under-load.txt, each excused only if its own
HERD-SKIPPED line names its ceiling and the load it measured (D196). Which
list was applied is in the verdict line and in herd-merger's report.

The board check, both ways, run because a batch with no conflict needs it
most. cut-v0.4.0 touched neither board.md nor status.md -- measured, not
assumed -- so there was no branch line to require verbatim, and the check
ran in the other direction instead: every one of the 139 ORCH rows on
origin/main is still present (140 now), every column of every changed row
keeps all of its old text as a prefix, three rows grew and one was added,
and no cell was shortened or reverted. ORCH-119 grew by 589 and 95
characters, ORCH-139 by 1040 and 190, ORCH-156 by 1009 and 110; ORCH-161
is new, eight columns, state open.

## Left out

- **The v0.4.0 tag, and the release.** Alexa has given the word for the
  release and not for the tag. The tag is his own separate act, to be run
  against this merge commit afterwards; no tag was created and none was
  pushed, and `git ls-remote --tags` was checked to say so. Landing a
  version branch never publishes its release.
- **Any pull request.** None was opened. The one exception in the method
  is spent and is not this landing.
- **ORCH-106 itself.** This landing writes down the operator's way round
  it and changes no code. The fix -- quarantine a torn record instead of
  refusing the whole root -- stays ORCH-106, owned by herd-step, and
  ORCH-161 closes the day it lands.
- **ORCH-139's actual fix.** The row is amended to say what must be done
  instead (run gate.sh bare); making the fixture's requirement explicit
  is still open and still herd-step's.
- **ORCH-145's reading**, the other half of ORCH-156. It is the
  consumer's to take, on their own root, and nobody here logs into it
  (D161, invariant 9).
- **Deleting cut-v0.4.0 or any other landed branch.** Not this task.

## Decisions allocated

Nothing. No entry headed D-NEXT exists anywhere in this batch -- checked
with git grep over the branch's whole docs tree, where every D-NEXT hit is
prose about the mechanism and not a header. The next free number is
therefore still D210: the highest on origin/main is D209, and D143, D154
and D170 stay gaps (D148, invariant 8).

The merger's own judgement, argued rather than asserted, because two
findings landed here and either could have earned a number. Neither does.
The test applied: a decision entry records a choice that constrains future
work and would otherwise be re-litigated. ORCH-139's amendment is a
measurement correcting a wrong diagnosis in a row that already exists and
already owns the fix -- it constrains nothing and a reader looking for it
looks at the row. ORCH-161 makes no choice at all: D199 already governs
the ordering and ORCH-106 already owns the behaviour; it records a
consequence of both. The release itself allocates none either, which is
herd-release's judgement and the orchestrator's, and the merger agrees:
D199 governs it.

## Board rows

- ORCH-156, owed -> owed, half discharged. The ordinary release is cut,
  gated and landed at layout 1 and record 1, which is the whole of what
  this row asks of a release. It is deliberately not `done`: shipped means
  tagged and published, and no v0.4.0 tag exists. Until it does, nothing
  of the version triple is briefed.
- ORCH-139, open, amended. The row's title blames a long TMPDIR and names
  one of two triggers. The rule is one line: run ./scripts/gate.sh BARE,
  never under nix develop --command, because gate.sh already wraps each
  toolchain step in nix develop itself (its lines 244, 246, 253) and
  nesting them nests TMPDIR from 45 to 62 characters, which with
  TempDir::new's ~53 bytes tips past the 108-byte SUN_LEN and panics
  confinement.rs:2529. Same sha RED nested, GREEN bare, measured by
  herd-release with TMPDIR already short.
- ORCH-119, done, amended with the fourth post-push run and the first
  green one: 40870735400 at 6036d24, success, which breaks the streak of
  three (40720983497, 40762764493, 40857453150) and is the first second
  machine to agree with D208. One green run is one sample and closes
  nothing; ORCH-160 stays open.
- ORCH-161, new, open. herd-hub report is the only way to read a state
  root that one torn job.json locks a hub out of, and v0.4.0's purpose is
  to ask an operator to open an old root. Re-verified on this tree, not at
  2ca6dc7 whose line numbers in ORCH-106 have drifted: store.rs:1339-1340,
  :1348-1349, :1358-1359 turn an unparseable job.json into Err while
  :1351-1354 quarantines a rejected label set; recover propagates at
  :1097, open_with at :481, and main at main.rs:358 -- 76 lines before
  Hub::bind_with at :434, so the hub exits before binding a port. report
  instead pushes it onto census.unread and continues
  (report.rs:334-341, :344-353). Owned by herd-release for every release
  body; closed by ORCH-106.

docs/status.md rewritten as state, not journal, still 90 of its 90 lines:
v0.4.0 cut and landed and not released, 334 commits and 84 merges past
v0.3.7, the three numbers unmoved, and the red streak broken.

## CI run

Recorded, not waited on: the id goes to herd-merger's report the moment
the push returns, from
`gh run list --workflow ci.yml --branch main --limit 1`, and the next
landing's board edit is what carries it into the repository. Not waited
on by choice, and the choice is argued: this batch touches no .rs file,
no Cargo dependency and nothing under .github, the local gate is green on
the identical tree, and the one post-push risk this landing carries --
ORCH-119's bounded race -- is the thing run 40870735400 has just come
back green on. A wait of about nine minutes is what the method exists to
stop paying. Its verdict is read at the next session start, and a failure
becomes an ORCH-nnn row before new work is briefed.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01FJ9b7vqezAbhLMw8LEsgsc
```

## Not done, and why

- **No tag and no release.** Alexa has given the word for the release and
  **not** for the tag; the tag is his own separate act, to be run against this
  merge commit afterwards. Creating one would exceed this agent's authority,
  and none exists locally or on the origin.
- **No pull request.** The method's single exception is spent and is not this
  landing.
- **ORCH-106 is not fixed.** This landing writes down the operator's way
  round it and changes no code. The fix — quarantine a torn record rather
  than refuse the whole root — stays ORCH-106, `herd-step`'s, with the
  `RECORD_VERSION`-mismatch caveat intact; ORCH-161 closes the day it lands.
- **ORCH-139 is amended, not fixed.** Making the fixture's requirement
  explicit — a short socket path of its own, or a named skip — is still open
  and still `herd-step`'s.
- **ORCH-145's reading** is not ours to take (D161, invariant 9).
- **`docs/status.md` does not mention ORCH-106 or ORCH-161.** A judgement,
  not an oversight: the file was at 90 of 90 lines with no slack, and
  `board.md` is the one canonical list of open work (invariant 8). If the
  orchestrator wants it in `status.md`, a line there must displace a real one.
- **`cut-v0.4.0` is not deleted**, nor any other landed branch.
- **Nothing was waited on.** Why, and the one risk it leaves, is in the
  record's *CI run* section.

## Risks

- **The post-push run on `main` is the only second opinion.** It was recorded
  and not waited on, and the argument is in the record: no `.rs` file, no
  dependency, nothing under `.github/`, and ORCH-119's bounded race — the one
  thing that reddened three runs in a row — has just come back green. If it
  is red anyway, `gh run rerun --failed` is step 1 and a failure becomes an
  `ORCH-nnn` row before new work is briefed.
- **ORCH-161 is prose and prose rots.** Nothing mechanical makes the next
  release body carry that caution, which is exactly why it is a board row
  with an owner and a closing condition rather than a sentence in one body.
- **The release body is evidence, not a published artefact.** It lives under
  `docs/orchestration/history/` and whoever publishes the release pastes it.
  This landing does not publish it, and if the tag is cut without reading the
  body, the caution reaches nobody.
- **One green CI run is one sample.** D208 is confirmed on a second machine,
  not proved; ORCH-160 is the survey that is still open.

## Resume point

`main` carries v0.4.0. **The next act is Alexa's: push the `v0.4.0` tag
against this landing's merge commit, then publish.** Nothing in this project
does either, and `herd-release` prepares and never publishes.

For the session that reads this:

1. **Read the post-push run.** Its id is in herd-merger's report to the
   orchestrator, not in this file — it cannot be, because this file is in the
   tree the gate ran on. `gh run list --workflow ci.yml --branch main
   --limit 5`. A `failure` becomes an `ORCH-nnn` row before any new work.
2. **The next landing's board edit records that run id**, as this one
   recorded batch 3's `40870735400`. That is the method's standing debt and it
   is one line.
3. **Open items this landing leaves on the board**, unchanged and in the
   order the board already ranks them: **ORCH-106** (briefed, the
   recommendation in its cell *is* the brief), **ORCH-161** behind it,
   **ORCH-139**, **ORCH-160**, **ORCH-155** and **ORCH-152** (the roadmap,
   which rewrites `plan.md` and needs a reviewer pass), **ORCH-148**,
   **ORCH-154**. Nothing was dropped because a batch ended.
4. **Next free numbers:** **D210** and **ORCH-162**.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01FJ9b7vqezAbhLMw8LEsgsc
