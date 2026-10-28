# method-rules — handoff 2026-10-28

- **Item:** ORCH-120 (answered 2026-10-28, then reaffirmed as an
  obligation the same day), with the two stale clauses ORCH-120's own
  row requires in the same edit
- **Agent:** herd-step
- **Branch / commit:** `method-rules`, based on origin/main `bc83717`;
  nine content commits, the last of them `360ee04`, plus the ones that
  carry this handoff — `git log --oneline origin/main..method-rules`
- **Landing:** none — this branch opens no pull request. The orchestrator
  assembles the one last pull request that carries the whole method
  change (*Not an option*, step 1); `herd-merger` names the landing here
- **Task:** T2 of the local-merge method (plan of 2026-10-28 by
  `herd-planner`), plus three mid-task course corrections and one
  adversarial review, all answered below. Prose only.

## Done

Write paths respected: `CLAUDE.md`, `docs/orchestration/workflow.md`,
the three agent role files, and this handoff. Nothing else — not
`docs/decisions.md`, not `board.md`, not `scripts/`, not `.claude/hooks/`
or `.claude/settings.json`.

The method is **in force**, in `CLAUDE.md` and in the role files:
nobody opens a pull request, a landing is one batch, one push, one
`--no-ff` merge commit carrying the landing record, one post-push run of
`ci.yml` on `main`, and `herd-merger` alone pushes `main` on a green
`./scripts/gate.sh` for that exact sha plus a filled landing record.
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

### Which sentences changed tense, and why

Commit 7 put the whole method in the conditional on the orchestrator's
instruction, after two of the plan's claims were measured false and a
full-workspace run at rest came back red. Commit 9 reversed that on
Alexa's reaffirmation. Every sentence that moved, both ways:

| Place | 7 made it | 9 made it |
|---|---|---|
| `CLAUDE.md` *Git and main* | origin/main's rule, plus a bullet saying what is *not in force yet* | the rule in force: no pull request ever, `gate.sh` for the exact sha, filled record, ORCH-001 re-granted |
| `workflow.md` subsection after the quote | *What is in force today, and what is proposed* | *Not an option: the obligation, and the transition to it* |
| the gate bullet | *"proposed and pending ORCH-nnn … today the gate is still CI"* | *"The gate is `./scripts/gate.sh`, green on the exact sha"* |
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

### The red suite is a blocker with an owner, not a condition

`workflow.md`, *Not an option*: a green `gate.sh` is **currently
unobtainable on this host** — a full-workspace run at rest fails
reproducibly, three times of three, on three `real_herd` 30 s timeouts
(`a_result_the_hub_cannot_write_is_kept_until_it_can`,
`a_drain_first_reservation_survives_a_kill_nine_on_either_side`,
`a_metal_job_drains_the_build_slot_first_and_no_build_overlaps_it`),
each of which passes when its target is run alone. Memory and IO
contention are both **refuted by measurement**, so it is a product
defect and an agent is hunting it. Written as a dated transition step:
the rule does not bend around it, the gate is not relaxed for it, the
defect is fixed and then the first landing happens. When the last
transition step is done, that subsection goes and nothing above it
changes.

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

Run after every edit of this task and after this handoff was committed;
identical every time. The `D170` line is a gap note, not a failure.

```
$ ./scripts/check-orchestration.sh | tail -2

check-orchestration: 146 passed, 0 failed
$ echo $?
0
```

**Five of the eleven runs of `check-orchestration.sh` were RED, and they
are reported red.** All five were the same single case, on trees whose
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

All five red runs were in shell invocations that also wrote files; all
six green ones ran the script alone, including two made immediately
after a red one on the same commit. So the verdict depends on how the invoking
shell was started, not on the repository. Outputs:

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

**Three references point at files not yet on `main`:**
`scripts/gate.sh` (on `method-local-gate`, head `58dd6f3`),
`templates/landing.md` (on `method-landing-record`) and
`scripts/githooks/pre-push` (being written now). The first two exist and
are named as existing. `pre-push` is named as existing too, on the
orchestrator's word that its task has started — **and it is the one
thing the safety argument leans on**, so the transition's step 4 makes
it an explicit precondition of the first landing, together with the
worktree's `core.hooksPath`. Without both there is no mechanism at all
in front of `main`, since the branch protection never bound our own
credential.

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

Four board edits this branch may not make (invariant 8):

1. **ORCH-120 answered 2026-10-28** and reaffirmed as an obligation the
   same day; the row still says `waiting` and still carries the
   recommendation the decision went against.
2. **`docs/orchestration/permissions.md` needs an owner.** Lines 5 and
   42-46 are true today and false the moment the protection is narrowed
   — one GitHub click — and line 46 is already weakened by
   `enforce_admins: false` independently of this branch. The row must
   exist before the click.
3. **Append to ORCH-111** the sample from this task: 5 red in 11 runs,
   same case, with the mechanism in *Gates* above. **Not a new row** —
   that would duplicate it and break invariant 8's one-list rule, which
   is what my previous handoff got wrong.
4. **A row for `CLAUDE.md` being at both ceilings**, 150 lines and 79
   columns, so that the next change to it is not briefed as if it had
   room.

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
> `./scripts/gate.sh` for the exact sha being pushed plus a filled
> landing record. Pushing a tag and publishing a release are unchanged:
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
> **The transition, and the one defect it waits on.** Four dated steps
> in `workflow.md`, *Not an option*; the first landing waits on a
> product defect — a full-workspace run at rest fails reproducibly on
> three `real_herd` 30 s timeouts that pass when run alone, with memory
> and IO contention refuted by measurement — which an agent is hunting.
> The rule does not bend around it.
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

`herd-reviewer` on this branch again, since the first review's verdict
was *must change* and all seven defects were answered on it rather than
on a new branch. The review's own minimum list is the checklist: D1's
three sentences, D2's preconditions, D3's step order, D5's revert, D6's
two restorations, D7's bound. Then the orchestrator assembles the one
last pull request of *Not an option* step 1.

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
