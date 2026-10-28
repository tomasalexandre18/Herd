# method-rules — handoff 2026-10-28

- **Item:** ORCH-120 (answered 2026-10-28, then reaffirmed as an
  obligation the same day), with the two stale clauses ORCH-120's own
  row requires in the same edit
- **Agent:** herd-step
- **Branch / commit:** `method-rules`, based on origin/main `bc83717`;
  ten content commits, the last of them `abfe6a1`, plus the ones that
  carry this handoff — `git log --oneline origin/main..method-rules`
- **Landing:** none — this branch opens no pull request. **`herd-merger`
  opens the one last pull request** that carries the method change, on
  the orchestrator's instruction (*Not an option*, step 1), and names
  the landing here. It **must land in the same batch** as
  `method-local-gate` (`scripts/gate.sh`), `method-landing-record`
  (`templates/landing.md`), the `pre-push` branch and
  `method-permissions-truth`: the contract this branch writes names all
  four, and three of them are not on `main`
- **Task:** T2 of the local-merge method (plan of 2026-10-28 by
  `herd-planner`), plus three mid-task course corrections and **two**
  adversarial review passes, all answered below. Prose only.

## Done

Write paths respected: `CLAUDE.md`, `docs/orchestration/workflow.md`,
the three agent role files, and this handoff. Nothing else — not
`docs/decisions.md`, not `board.md`, not `scripts/`, not `.claude/hooks/`
or `.claude/settings.json`.

The method is **in force**, in `CLAUDE.md` and in the role files:
nobody opens a pull request, a landing is one batch, one push, one
`--no-ff` merge commit carrying the landing record, one post-push run of
`ci.yml` on `main`, and `herd-merger` alone pushes `main` on a green
`./scripts/gate.sh` for the **gated** sha — the merge's second parent,
trees identical — plus a filled landing record.
Alexa, 2026-10-28, verbatim and quoted in `workflow.md`: *"que l'on soit
clair, l'abandon des PR et CI Github n'est pas une option, mais une
obligation."*

### The commits

1. **Replace one PR per batch with one landing per batch** — the section
   *One PR per batch of branches* becomes *One landing per batch*, with
   Alexa's words of 2026-10-28 verbatim, the answer to **ORCH-120**
   against the recommendation in that row and wider than its fallback,
   and *What a landing is*: the one-to-one chain that keeps a red run
   attributable. The old section title is named in the text so older
   board rows and handoffs still find it.
2. **Mark the local gate adopted and fix two stale clauses** — the
   refusal of 2026-10-27 kept, marked **ADOPTED 2026-10-28**, its
   reasoning quoted as written with its two factual errors marked in
   place, and the corrected figures beside it: **12 here at rest, 14
   here under load, 14 on CI** (ORCH-116, run 40538722315). The *"15 on
   CI"* came from the *"AT LEAST 15"* of red run 40439537929, which is
   what D179 exists to label.
3. **Write down what the local merge costs** — now ten items, including
   the three measurements that corrected claims made in our own favour.
4. **Rewrite Git and main for the local landing.** Undone by 7, redone
   by 9; the diffs are left in the log because the reversal is the
   record.
5. **Update the three agent roles.**
6. **Tie the nightly refusal to the workflow file that would end it.**
7. **Hold the new merge condition pending the flaky-suite finding** —
   reversed by 9, kept in the log.
8. **Give `herd-merger` a closing fast-forward of the orchestrator's
   checkout.**
9. **Put the new method in force and answer the review.**
10. **Make the merge condition satisfiable and route the last pull
    request** — the pass-2 review, below.

### Which sentences changed tense, and why

Commit 7 put the whole method in the conditional on the orchestrator's
instruction, after two of the plan's claims were measured false and a
full-workspace run at rest came back red. Commit 9 reversed that on
Alexa's reaffirmation. Every sentence that moved, both ways:

| Place | 7 made it | 9 made it |
|---|---|---|
| `CLAUDE.md` *Git and main* | origin/main's rule, plus a bullet saying what is *not in force yet* | the rule in force: no pull request but the one that lands this change, `gate.sh` for the gated sha, filled record, ORCH-001 re-granted |
| `workflow.md` subsection after the quote | *What is in force today, and what is proposed* | *Not an option: the obligation, and the transition to it* |
| the gate bullet | *"proposed and pending ORCH-nnn … today the gate is still CI"* | *"The gate is `./scripts/gate.sh`, green on the sha it gated"* (commit 10 corrected "the exact sha that is pushed", which no landing could satisfy) |
| *Transition* | two blockers, the second being what a green gate proves | four dated steps plus the one defect step 4 waits on |
| loop step 7 | both lanes, today's first | one lane |
| *Permissions and escalation* | in force / proposed, separately | one condition, the old one named as gone |
| the ADOPTED refusal | *"adopted in principle and not yet in force"* | adopted, reaffirmed as an obligation, pointing at the one defect |
| `herd-merger.md` intro, gate step, merge step, handoff list | a lane split and "do not switch lanes" | one lane; the orchestrator alone may route a landing through the last pull request |
| `herd-step.md` gates bullet | *"CI's today and `gate.sh`'s once in force"* | *"the batch's own `gate.sh` run at integration"* |

**What did not change tense, deliberately:** every caveat. The three
corrected measurements and the whole of *What the local merge costs* are
word for word what commit 7 wrote; commit 9 only reclassified them with
one new lead-in — **known limitations of the method we are adopting, not
open questions about whether to adopt it**. Run 40439537929 is now
stated as the argument *for* the reading duties: it went unread while
the pull request gate was in front of it, so removing that gate does not
create the problem, it removes the cushion that made ignoring it
survivable.

### The pass-2 review, defect by defect

The seven pass-1 answers were re-measured and all held. The one-lane
rewrite had created three new blocking defects.

- **N1, and it was the worst: the merge condition was unsatisfiable.**
  Five places required a green gate for *the exact sha that is pushed*.
  The gate runs on the integration tip; the `--no-ff` merge commit is
  created after it and is what gets pushed, and its message **is** the
  record, so it cannot exist before the gate. Measured: gated
  `31bb327f…`, pushed `d791dfbc…`, same tree, different sha. Written to
  the letter, **no landing was ever permitted** — and `pre-push` is
  being built against this text. All five now say the condition is a
  receipt for **the merge's second parent** plus an **identical tree**,
  and *What a landing is* carries the check itself
  (`git rev-parse "$local_sha^2"`, `git diff --quiet`) so the hook has
  something exact to implement.
- **N2: the one last pull request had no actor.** It was forbidden in
  four places and permitted in one, and my own previous handoff named
  the orchestrator — the one actor that cannot, since
  `orchestrator-guard.sh` denies it `gh pr create` (exit 2, measured).
  **`herd-merger` opens it**, on the orchestrator's instruction, for
  that landing only, dated 2026-10-28, and the same clause now appears
  in all four prohibitions: `CLAUDE.md`'s *One landing per batch*
  bullet, `workflow.md`'s *The pull request goes away*, the role table,
  and `herd-merger.md`'s *Forbidden*. `herd-step` and `herd-release` say
  the exception is the merger's alone and never theirs.
- **N3: transition step 3 could not land in the order it was written.**
  `ci.yml` triggers on `pull_request:` and `push: main`, so a pull
  request deleting its own `pull_request:` trigger produces no `check`
  and no `docs` and can never satisfy the required contexts; and the
  step was numbered **before** the first local landing, so following
  1→2→3 moved `main` with no pull request, no hook and no receipt rule.
  The transition is now **three steps**: step 1's pull request carries
  the template deletion only, and the trigger deletion is the content of
  the first local landing, the only lane that can do it. A paragraph
  after step 3 says why there are three and not four.
- **N4:** step 3's preconditions now list `scripts/gate.sh`,
  `docs/orchestration/templates/landing.md` and
  `scripts/githooks/pre-push`, and say in the text that they are on
  pushed branches of this batch and **not on `main`** — this file and
  they land together, or the contract names three things that do not
  exist.
- **N5:** the per-worktree hooks-path line is gone from the loop's step
  4. `core.hooksPath` is written as **one repository-wide setting** and
  the second named exception to *write only in your own worktree*, set
  as part of the first landing and **guarded on the hook file existing**
  — measured: `git -C <worktree> config` lands in the shared config of
  `/mnt/ssd_980/herd-project` and arms all 85 worktrees at once,
  `--worktree` is refused because `extensions.worktreeConfig` is unset,
  and pointing `core.hooksPath` at a missing directory disables
  `.git/hooks` and supplies nothing. `CLAUDE.md`'s bullet now says
  *"the two exceptions, with their conditions, are in workflow.md"*
  rather than naming one.
- **N7:** the claim that every "refused server-side" sentence **is**
  rewritten is narrowed to the three files this branch owns, and says
  plainly that `permissions.md:46` still contradicts them, that it is
  corrected on `method-permissions-truth`, and that the two land in the
  same batch — until both are on `main` the project states two
  different things about the same protection.
- **N8:** the fast-forward block prints why it skipped and the before
  and after shas; all three cases exit 0, so a silent skip was
  indistinguishable from a success.
- **N9:** the loop's step 7 now says the merger's handoff is **committed
  before the gate**, which is the whole of its ORCH-121 answer and was
  only in the role file.
- **N10:** the "stale by construction" bullet now carries the `git -C`
  caveat and points at *Unchanged* for the hole; and
  `.claude/agents/herd-planner.md:23` gives `gh run view` instead of
  `gh pr view`. **That file was not in my write paths** — I edited it on
  the orchestrator's explicit instruction in the same message that sent
  this review, and it is a one-word change.

### The red suite was a blocker; it is fixed

Until the morning of 2026-10-28 a green `gate.sh` was unobtainable on
this host: a full-workspace run at rest failed reproducibly, three times
of three, on three `real_herd` 30 s timeouts
(`a_result_the_hub_cannot_write_is_kept_until_it_can`,
`a_drain_first_reservation_survives_a_kill_nine_on_either_side`,
`a_metal_job_drains_the_build_slot_first_and_no_build_overlaps_it`),
each of which passed when its target was run alone. I wrote it into
`workflow.md` as a dated transition blocker with an owner.

**It was then found and fixed the same day, and it was a test defect,
not a product one.** The runner announced **one job slot**, because
D119's cgroup clamp cut the announced room to 4.6 GiB, and each of those
three fixtures silently needed two concurrent jobs; the hub had been
printing the reason in words all along. One declared constant in the
three tests, `--job-memory-max=512MiB`, on branch
`real-herd-patience-timeouts`. **The full suite is now green at rest: 0
failed, 12 skips, `test suite: PASSED`.** So `workflow.md` no longer
carries a blocker: it carries the defect, named with its three test
names and its cause, and says the first landing does not wait on it.
A blocker that no longer exists is worse than no text at all, and the
sentence that said *"an agent is hunting it"* was the kind of owner a
next session cannot find.

The fix wants its **own board row**, which `herd-merger` allocates; the
product-defect row the first review asked for is **not** needed, because
there was no product defect.

### The review's blocking defects, each answered

- **D1 — "refused server-side" was false.** No sentence claims it now.
  The `gh api …/protection` output, `gh api user` and the `admin: true`
  permissions are quoted in *What the local merge costs* as their own
  cost item, with the consequence stated: `allow_force_pushes: false`
  and `allow_deletions: false` bind everyone **except** the account that
  would push, because `enforce_admins` is false. The landing record's
  permanence is now **contractual**: the record is better than a pull
  request body because it is attached to the thing it documents and
  reads with no network, **not** because a server protects it. The
  transition's step 2 says in bold that narrowing the protection is a
  tidying step and not a safety one. Turning `enforce_admins` on is
  named as Alexa's act that would make the old sentences true.
- **D2 — step 9's safety was asserted.** It is step 10 now and its
  preconditions are **commands**: HEAD is `main`, `status --porcelain`
  empty, then `fetch` and `merge --ff-only`. Both measured failure modes
  are written down — a dirty file the incoming commits do not touch is
  carried straight through, and with another branch checked out the
  merge silently fast-forwards *that* branch while local `main` never
  moves — and the sentence claiming dirty makes `--ff-only` fail is gone
  from both files. `fetch origin main:main` is named as not a
  substitute.
- **D3 — a write after the push.** Resolved by ordering: the landing
  record at step 5, the **handoff written and committed at step 6**, the
  gate at step 7 as the last thing before the merge, the merge and the
  one push at step 8. The post-push run id goes to the merger's
  **report** and the **next** landing's board edit; `workflow.md` says
  why in its own bullet — the gate must run on the tree that is pushed,
  so nothing can be added to that tree afterwards, and two pushes for
  one batch is the one thing the section forbids. The claim that the
  gate's output lands in the same push is gone; what lands is the
  handoffs.
- **D4 — dissolved by the single lane.** There is one template,
  `templates/landing.md`, which exists on `method-landing-record`. The
  only pull request left is the transition's step 1, whose body is that
  record; `.github/pull_request_template.md` is named there as what any
  still-open pull request was written from, and step 3 deletes it.
- **D5 — the revert shortcut.** *"A revert needs no pull request and
  never did"* is deleted. The step now reads that a revert is an
  ordinary landing and gets no shortcut, and that history stays
  append-only because we keep it so and `pre-push` refuses the rest.
- **D6 — `CLAUDE.md` said what was not in force.** It now says what is.
  And `herd-step`'s row in `workflow.md` has **"open a pull request"**
  restored and **"touch `main`"** back in place of the narrower "push to
  `main`".
- **D7 — the unbounded exception.** `CLAUDE.md` reads *"`herd-merger`'s
  closing `--ff-only`, on workflow.md's conditions, excepted"*, so the
  licence carries its bound. `workflow.md` gains a bullet saying
  creating a worktree from that checkout and fetching into it are **not**
  writes in the sense of the rule — `git worktree add` registers
  metadata and `git fetch` writes `FETCH_HEAD`, and every brief asks for
  both — and the merger's *Forbidden* list says the same. The stale
  justification is corrected: the guard denies the **merge**, not the
  fetch. And *Unchanged* no longer claims enforcement: the orchestrator
  not merging or pushing is **contractual**, because
  `orchestrator-guard.sh` was probed and lets `git -C <dir> merge` and
  `git -C <dir> push origin main` through. The hole is named as the push
  guard's task.
- **Smaller ones.** The broken wrap is fixed. *"The suite is no longer
  red here for host reasons"* now carries *"which is not the same as
  green: a full-workspace run at rest was red on 2026-10-28"*. The
  `ORCH-nnn` overload is resolved by the single lane: four uses remain,
  all the generic *"a new row gets an `ORCH-nnn` id"* idiom, and `D-A`
  appears nowhere.

### Budget evidence, both required by the brief

```
$ wc -l CLAUDE.md
150 CLAUDE.md
$ diff <(git show origin/main:CLAUDE.md | sed -n '/^## Invariants/,/^## Layout/p') \
       <(sed -n '/^## Invariants/,/^## Layout/p' CLAUDE.md)
$ echo $?
0
```

Every line `CLAUDE.md` gained was paid for by a substitution of equal or
smaller length, never by shortening an invariant: the four-line *One PR
per batch* bullet became a three-line *One landing per batch* bullet and
a five-line *Pushing to `main`* bullet replaced the two bullets
(*"CI must be green"*, *"Merging is `herd-merger`'s"*) it supersedes;
the freed line went to the *Shared host rules* exception. The overflow
went to `workflow.md`: ORCH-001's verbatim words, the `--no-verify`
reason, the full `gh run list` command, the session-start reading duty
and every measurement.

## Not done, and why

- **`docs/decisions.md` is untouched** (invariant 8: only `herd-merger`
  appends). The entry is proposed below.
- **`docs/orchestration/board.md` is untouched.** Four rows are owed at
  integration; they are listed under *For `herd-merger` in this
  landing*.
- **The session-start reading duty is not in `CLAUDE.md`.** It is in
  `workflow.md` only, because the file has zero budget. It is the
  mitigation for a loss that has already happened once, so it is the
  first thing to put back if a line is ever freed.
- **No Rust gate was run and no toolchain was used.** Prose only.
- **`docs/orchestration/permissions.md` is untouched** and is wrong the
  moment the protection is narrowed. It is outside my write paths and
  nobody owns it; see below.

## Gates

Prose-only change, so the **four Rust gates were NOT run locally**:
`cargo build`, `cargo clippy`, `cargo fmt --check` and
`./scripts/test.sh` were not executed by this task. Under the method
this branch writes, what covers them is the batch's own
`./scripts/gate.sh` run at integration, which is `herd-merger`'s.

```
$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
$ echo $?
0
```

Run after every edit of this task and after this handoff was committed.
It was green every time **but once**, and that once is worth recording
because it is the rule this branch writes, applied to the branch itself:
after the pass-2 commit I ran the gate and pushed in the same breath,
and the gate came back red on an 80-column line in this file — one line
over, in the very section that says a gate's output is what you read.
The push had already gone. The fix is its own commit, `7c639ba`'s
successor, with the gate green on the tree that is now the head. Under
the method this branch writes, `pre-push` would have refused that push
for want of a receipt; nothing refused it here, which is the honest
measure of what the contract alone is worth. The `D170` line is a gap
note, not a failure.

```
$ ./scripts/check-orchestration.sh | tail -2

check-orchestration: 146 passed, 0 failed
$ echo $?
0
```

**Seven of the fourteen runs of `check-orchestration.sh` were RED, and
they are reported red.** All seven were the same single case, on trees
whose
executable files are byte-identical to `origin/main`:

```
FAIL  PATH dropped the inherited entries; the guard below needs coreutils
    …/stateroot/claude/tools/dtach/bin:/run/wrappers/bin:/etc/profiles/per-user/server/bin:/run/current-system/sw/bin:/wrong/home/.local/bin:/run/wrappers/bin:/wrong/home/.nix-profile/bin:/nix/profile/bin:/wrong/home/.local/state/nix/profile/bin:/etc/profiles/per-user//bin:/nix/var/nix/profiles/default/bin:/run/current-system/sw/bin

check-orchestration: 145 passed, 1 failed
```

**This is ORCH-111**, already open at `board.md:111` with a "1 red in 6
runs" sample from batch 4 — same case, same shape of captured output.
The line number moved from 438 to 464 as the script grew. **It is not
this diff:** `git diff --stat origin/main -- scripts/ ops/
.claude/hooks/ .claude/settings.json` is empty, and two green runs
predate any edit of mine.

What my runs add to that row: the case is
`scripts/check-orchestration.sh:464`, which sources `ops/agents/env.sh`
under `env -i PATH="$PATH" HOME=/wrong/home` and asserts the caller's
**literal** `$PATH` is still a substring of what came back. In the red
runs what came back was rebuilt from `HOME` instead — note
`/wrong/home/.local/bin` and `/etc/profiles/per-user//bin` with an empty
user. Run by hand with my own `$PATH` the same inner command passes:

```
$ out=$(env -i PATH="$PATH" HOME=/wrong/home bash -c '. ops/agents/env.sh >/dev/null 2>&1; printf "%s" "$PATH"')
$ printf '%s' "$out" | grep -qF ":$PATH" && echo PRESENT
PRESENT
```

Every red run was in a shell invocation that also wrote files; every
green one ran the script alone, including three made immediately after a
red one on the same commit. Fourteen runs, seven red, seven green, and
the split is exactly along that line. So the verdict depends on how the
invoking shell was started, not on the repository. Outputs:

```
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/method-rules/out/
```

No skip: neither script has a skip mechanism and no `HERD-SKIPPED` line
appears in any output. Whole-script timing: **12 s** before my edits,
**12-13 s** after; no case added or removed.

**The brief expected a red `check-docs.sh` complaining about `D-NEXT`**
(gotcha `a-decision-branch-cannot-be-all-green`). It is green, and
consistently: the same brief forbade writing `docs/decisions.md`, so
there is no placeholder. The gotcha does not apply to a branch that
proposes its decision in its handoff.

**And a gap no gate can close, written into `workflow.md` where a reader
meets it:** `check-docs.sh` and `check-orchestration.sh` can only ever
see the landing record's **copy in the handoff**, never the real merge
commit message, because `check-orchestration.sh` builds each corpus by
copying the tracked files **without `.git`**. A green docs gate says the
handoff carries a filled record and says nothing about what
`git log -1 --format=%B` on `main` holds. Only
`scripts/githooks/pre-push` can compare the two; until it does, that
comparison is a human's.

## Risks and findings

**Where the brief is wrong.**

1. **The `CLAUDE.md` line budget does not exist.** The plan claimed 8-12
   lines of rewrap headroom, 4 inside *Git and main*. Measured block by
   block over `origin/main` — every paragraph and bullet outside the
   Invariants, rewrapped at 79 columns with its continuation indent —
   the theoretical gain is **zero**. The reviewer confirmed it and added
   the second ceiling: the file's widest line is 79, the cap is 79. So
   both dimensions are at the limit and **every future line added to
   `CLAUDE.md` must be paid by an equal deletion**. The plan's **H7 is
   refuted** and this deserves a board row of its own.
2. **The board row is ORCH-120, not ORCH-032.** The "two stale clauses
   in one edit" requirement is in row ORCH-120, at *line 32* of
   `board.md` — which is where the id came from. ORCH-032 is "Live
   validation V2…V10", done 2026-10-24.
3. **The `D-NEXT` expectation contradicts the brief's own write paths.**
4. **"the local suite has been green" was never verified by me**, and
   the measurement since says a run at rest is red. What I wrote is the
   narrower claim the record supports — *no longer red for host
   reasons*, the two host-dependent causes being fixed — with the
   red-at-rest caveat attached to it in the same sentence.
5. **A correction to my own earlier handoff:** it claimed "two lines
   gained, nothing deleted" in `CLAUDE.md`. The reviewer measured net
   **zero**, and it was right. The present version does delete two
   bullets — *"CI must be green before merge"* and the old *"Merging is
   `herd-merger`'s"* — and that is deliberate: they are the rule the
   method replaces, not content lost.

**This branch cannot land alone.** The contract it writes names four
things that are not on `main`: `scripts/gate.sh`
(`method-local-gate`, head `58dd6f3`), `templates/landing.md`
(`method-landing-record`), `scripts/githooks/pre-push` (being written),
and the corrected `permissions.md` (`method-permissions-truth`). All
four are named in the present indicative, which is correct under the
single lane and makes the batch order load-bearing rather than
cosmetic: the transition's step 3 lists the first three as explicit
preconditions of the first landing, together with the repository-wide
`core.hooksPath`. Without the hook and that setting there is no
mechanism at all in front of `main`, since the branch protection never
bound our own credential.

**One contradiction I could not remove.** The third refusal still says a
nightly full pass is not adopted, true today, while another task adds a
`schedule:` trigger to `ci.yml`. I was told to mark only the second
refusal adopted, so the clause says instead that if a scheduled run is
added it is updated in the same landing.

**What I chose, and what I rejected.**

- I kept the whole history in the section and in the log: both quotes
  dated, the old section title named, commits 4 and 7 left in although 9
  undid them. Rejected: a tidy page and a rebased branch; a record of a
  reversal is worth more.
- I marked the refusal's two errors **inside** the quotation. Rejected:
  correcting the quote silently, and paraphrasing a refusal of Alexa's.
- I reclassified the caveats rather than softening them, and added a
  tenth cost item for the protection measurement rather than deleting
  the sentences it falsified. Rejected: dropping "refused server-side"
  quietly, which would have left the next reader to re-measure it.
- I made the merger's handoff precede the gate and kept the gate last.
  Rejected: letting the handoff quote a run that happens after it, which
  is the ORCH-121 defect with extra steps.
- I put the post-push run id in the report rather than the board.
  Rejected: a second commit and a second push, which the same section
  calls the thing that destroys attributability.

## For `herd-merger` in this landing

Five board edits this branch may not make (invariant 8). The first is
the one that cannot wait: `CLAUDE.md`'s session start sends every fresh
session to the board at step 2, where it currently reads a waiting
decision proposing the opposite of the contract it has just read.

1. **Close ORCH-120** — answered 2026-10-28 and reaffirmed as an
   obligation the same day. The row still says `waiting` and still
   carries the recommendation the decision went against.
2. **A row for the test defect that was fixed**, naming the three
   `real_herd` test names, the three-of-three measurement, the cause
   (one announced job slot against fixtures needing two) and the fix
   (`--job-memory-max=512MiB`, branch
   `real-herd-patience-timeouts`), with the suite green at rest.
3. **Append to ORCH-111** the sample from this task: 7 red in 14 runs,
   same case, with the mechanism in *Gates* above. **Not a new row** —
   that would duplicate it and break invariant 8's one-list rule, which
   is what an earlier version of this handoff got wrong.
4. **A row for `CLAUDE.md` being at both ceilings**, 150 lines and 79
   columns, so that the next change to it is not briefed as if it had
   room.
5. **`permissions.md`** no longer needs an owner from this landing — it
   has one, `method-permissions-truth`, in this batch. What it needs is
   the row that records the two landing **together**, because between
   them the project would state two different things about the same
   protection.

## Decision to record (proposed, for `herd-merger` to allocate)

One entry, not two: the hold that justified splitting it was reversed.

> `## 2026-10-28 — D-NEXT The pull request leaves the loop: one landing
> per batch, gated locally`
>
> **Decision (Alexa, 2026-10-28).** First: *"en gros je verais juste la
> supresions des pr et sortir de l'env Github … un agent peux tester en
> local et le merge aussi avant d'envoyer vers main, Github est juste un
> centre de stokage … (regles aussi les problemme des 2000 minute de
> run)"*. Then, when the orchestrator put the objection that a local
> gate is one machine and that this one could not produce a green run:
> *"que l'on soit clair, l'abandon des PR et CI Github n'est pas une
> option, mais une obligation."* The pull request leaves the loop. A
> landing is one batch, one push, one `--no-ff` merge commit whose
> message is the landing record, one post-push run of `ci.yml` on
> `main` — one-to-one, because that chain is what keeps a red run
> attributable. Six runs on `main` were `cancelled` in the five days
> before, three within seven seconds of each other.
>
> **ORCH-001 re-granted, not reinterpreted.** Both halves of the
> 2026-10-24 condition — CI green, Evidence block filled — are gone with
> the pull request. `herd-merger` alone pushes `main`, on a green
> `./scripts/gate.sh` for the sha it gated — **the merge's second
> parent, with an identical tree**, because the merge commit's message
> is the record and so its sha cannot exist before the gate; written as
> "the exact sha that is pushed", as it was until this was measured, no
> landing is ever permitted. Pushing a tag and publishing a release are
> unchanged:
> Alexa's word, one act at a time.
>
> **What it answers.** ORCH-120, against the recommendation in that row
> (*"not yet — keep CI as the gate"*) and wider than its fallback. The
> refusal of 2026-10-27 made itself reconsiderable once ORCH-069 and
> ORCH-107 were fixed; both landed, so this is that sequence arriving.
>
> **Nothing waits for a review.** Asked whether rule changes
> (`CLAUDE.md`, the invariants, `.claude/`, a release cut) should keep a
> pull request so he could look before they land, Alexa answered no:
> everything lands locally, rules included, and he reads `main` when it
> suits him. A reduction in oversight, recorded as one.
>
> **Known limitations, measured the same day and not reasons to wait.**
> Neither host runs the twelve confinement assertions behind invariant 2
> — all twelve skips here are the same cgroup-delegation refusal, and
> this host runs **two** assertions CI cannot, which is the whole of the
> 12-against-14 difference. The gate measures a **cgroup slice**, not a
> host: 4.9 GiB announced rather than 31.1 GiB, because
> `/chatons.slice`'s `memory.high` is 18 GiB and already held 13 GiB, so
> two agents in different slices do not run the same suite. And `main`'s
> history is protected **contractually**, not server-side:
> `enforce_admins` is false and our only push credential is a repository
> admin, so the branch protection does not bind the account that pushes.
>
> **What this does not decide.** The branch protection change, the
> `ci.yml` triggers, `gate.sh`'s contents, the `pre-push` hook, whether
> a nightly run is adopted, and whether `enforce_admins` is turned on.
> `release.yml` and `release-dev.yml` are untouched and are never
> deleted: GitHub remains the release factory (D162, D168), the hotfix
> lane (D172) and our only second machine.
>
> **The transition.** Three dated steps in `workflow.md`, *Not an
> option*: one last pull request, opened by `herd-merger` on the
> orchestrator's instruction, carrying the method change and the
> template deletion; the branch protection narrowed; then the first
> local landing, which is the only lane that can delete `ci.yml`'s
> `pull_request:` trigger, since a pull request removing that trigger
> can never produce the contexts it requires. The red suite that looked
> like a blocker was a **test** defect — three fixtures needing two job
> slots while the runner announced one — fixed the same day, and the
> suite is green at rest.
>
> **Reversibility.** High on our side, low on Alexa's: the text is a
> landing away, but re-requiring a pull request means putting the branch
> protection back, which is his act.

## Decisions needed

1. **Whether `enforce_admins` is turned on.** *Question:* the branch
   protection on `main` does not bind our own admin credential, so
   `main`'s history is protected by contract and by a client-side hook
   that `--no-verify` bypasses. Turning `enforce_admins` on would make
   it mechanical again. *Recommendation:* ask Alexa — it is a GitHub
   account setting and outward-facing, and it is the cheapest mechanism
   left after the pull request goes. *Fallback:* the contract plus
   `pre-push`, which is what the text now claims and nothing more.
2. **Who fixes `docs/orchestration/permissions.md`** (above).
3. **The wording of the re-granted ORCH-001** (the plan's D-2). It is
   now written as in force in three files. *Recommendation:* let it
   stand; his words cover it. *Fallback:* he corrects it when he reads
   `main`, and a later entry records the correction.

## Resume point

`herd-reviewer`, third pass on this branch: both earlier verdicts were
*must change* and every defect of both was answered on this same branch
rather than on a new one, so the two review files are the checklist.
Pass 2's minimum list in order: **N1** the condition names the merge's
second parent and an identical tree in all five places — `CLAUDE.md`'s
*Pushing to `main`* bullet, `workflow.md`'s loop step 7, its gate
bullet, its *Permissions* bullet and `herd-merger.md` steps 7 and 8;
**N2** the identical exception clause in all four prohibitions, with
`herd-merger` as the actor; **N3** three transition steps with the
trigger deletion inside the first landing; **N4** the preconditions and
the batch order; **N5** the repository-wide hooks path, guarded;
**N7** the narrowed claim; **N8**, **N9**, **N10**.

Then `herd-merger` assembles the one last pull request of *Not an
option* step 1 — it is the actor now, on the orchestrator's instruction
— with this branch and the four it names in the same batch.

## Processes

No process started, no PID, no background job, no compile, no `cargo`.
No `target/` was created, so none was deleted. The only writes were in
my own worktree and my own scratchpad subdirectory:

```
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/method-rules/wt
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/method-rules/out
```

The orchestrator's checkout was read with `git -C … fetch` twice, to
create this worktree and to refresh `origin/main`, and never written.
