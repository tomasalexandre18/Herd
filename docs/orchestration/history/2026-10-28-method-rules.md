# method-rules — handoff 2026-10-28

- **Item:** ORCH-120 (answered 2026-10-28), with the two stale clauses
  ORCH-120's own row requires in the same edit
- **Agent:** herd-step
- **Branch / commit:** `method-rules`, based on origin/main `bc83717`;
  eight content commits, the last of them `8a6287c`, plus this handoff's
  own — `git log --oneline origin/main..method-rules` is the list
- **Landing:** none — this branch is pushed and opens no pull request;
  it is integrated with its batch, and `herd-merger` names the landing
  here
- **Task:** T2 of the local-merge method (plan of 2026-10-28 by
  `herd-planner`, in that agent's scratchpad), plus two mid-task
  corrections from the orchestrator, both recorded below. Prose only.

## Done

Eight content commits, each one idea, plus the two that carry this
handoff. Write paths respected:
`CLAUDE.md`, `docs/orchestration/workflow.md`, the three agent files,
and this handoff. Nothing else.

1. **Replace one PR per batch with one landing per batch**
   (`workflow.md`). The section *One PR per batch of branches* becomes
   *One landing per batch*, with Alexa's words of 2026-10-28 verbatim
   and the statement that they answer **ORCH-120** against the
   recommendation in that row and wider than its fallback. A new
   subsection *What a landing is* defines the unit: one batch, one push,
   one `--no-ff` merge commit whose message is the landing record, one
   post-push run of `ci.yml` on `main` — one-to-one, because that chain
   is what keeps a red run attributable. The old section title is named
   in the text so that board rows and handoffs pointing at it still
   find the section.
2. **Mark the local gate adopted and fix two stale clauses.** The
   refusal of 2026-10-27 is marked **ADOPTED 2026-10-28** and kept, not
   deleted; its reasoning is quoted as written with its two factual
   errors marked in place (*"the suite is red here for host reasons"* —
   no longer true; *"12 here against 15 on CI"* — the 15 is wrong), and
   the corrected figures follow: **12 here at rest, 14 here under load,
   14 on CI** (ORCH-116, run 40538722315), the 15 being the *"AT LEAST
   15"* of red run 40439537929, which is what D179 exists to label. The
   other two refusals keep their date and reason.
3. **Write down what the local merge costs** — now nine items in
   `workflow.md`, including the two corrected measurements of item 7
   below — plus a procedure for a red post-push run on `main`.
4. **Rewrite Git and main for the local landing** (`CLAUDE.md`). **This
   commit was largely undone by commit 7**; the diff is kept because the
   reversal is worth reading.
5. **Update the three agent roles.**
6. **Tie the nightly refusal to the workflow file that would end it.**
7. **Hold the new merge condition pending the flaky-suite finding.**
8. **Give `herd-merger` a closing fast-forward of the orchestrator's
   checkout.**

### What commits 7 and 8 changed, and why

**Commit 7 is the first mid-task correction.** Two claims I had taken
from the plan were measured false on 2026-10-28 by the agent that built
`scripts/gate.sh`, and a full-workspace run at rest on this workstation
came back **red**: three `real_herd` 30 s timeouts
(`a_result_the_hub_cannot_write_is_kept_until_it_can`,
`a_drain_first_reservation_survives_a_kill_nine_on_either_side`,
`a_metal_job_drains_the_build_slot_first_and_no_build_overlaps_it`) that
pass when their targets are run alone. So the sentences I had written
making a green local gate the merge condition came out. Exactly what
changed:

- **`CLAUDE.md` is back to `origin/main`'s rule.** *"CI must be green
  before merge: jobs `check` and `docs`"* and ORCH-001 as granted on
  2026-10-24 (*"any PR whose CI is green and whose Evidence block is
  filled"*) stand untouched. The new condition is **not** in
  `CLAUDE.md`. Two lines are all it gains: the *One PR per batch*
  bullet becomes a *One landing per batch* bullet marked **"Not in
  force yet"** pointing at `workflow.md`, and the *Write only in your
  own worktree* bullet gains the one exception commit 8 needs.
- **`workflow.md` gains *What is in force today, and what is
  proposed***, which says plainly: in force today, one pull request per
  batch and CI as the gate; proposed and pending **ORCH-nnn (D-A)**, a
  green `gate.sh` for the exact sha plus a filled landing record.
  **Alexa's grant is not what is in doubt** — he re-granted ORCH-001
  this morning — what is in doubt is whether a green `gate.sh` means
  anything yet, and the red run at rest is quoted as the reason.
- Four more places were put back in the same tense: the gate bullet is
  marked *proposed and pending*, the *Transition* bullet now names two
  blockers in order (the branch protection, then ORCH-nnn), the loop's
  step 7 describes **both** lanes with today's first, and *Permissions
  and escalation* states the in-force condition and the proposed one
  separately. The refusal marked ADOPTED now reads *"adopted in
  principle and not yet in force"*.
- `herd-merger.md` has a lane split in its intro, its gate step, its
  merge step and its handoff list, and is told not to switch lanes on
  its own judgement, because that switch is ORCH-nnn plus a GitHub
  settings change and neither is an agent's.
- The two corrected facts are in *What the local merge costs*, in the
  narrow form I was given and with the date: **neither host runs the
  twelve confinement assertions** — all twelve skips here are the same
  *"cgroup v2 delegation is unavailable … (Permission denied (os error
  13))"*, so the dramatic version (*"CI structurally cannot check the
  confinement assertions and this host can"*) is **false**; the real
  12-against-14 difference is two CI-only skips, both
  `the_io_weight_verdict_follows_the_device_the_job_works_on`, which do
  not skip here because `/mnt/ssd_980` has blk-iocost — and **the gate
  measures a cgroup slice, not a host**, with the runner's own
  *"4.9 GiB rather than 31.1 GiB, because /proc/self/cgroup is enclosed
  by memory.high 18 GiB on /chatons.slice, which already holds 13 GiB"*.
  Two agents in different slices do not run the same suite.

**Commit 8 is the second mid-task correction**, and it depends on none
of the above. `herd-merger` gets a step 9, the last thing it does once
`main` has moved: `git -C /mnt/ssd_980/herd-project fetch origin` then
`merge --ff-only origin/main`. Fast-forward only; a dirty or diverged
checkout is reported in the handoff and **left alone** — never `reset
--hard`, never `checkout -f`, never `stash`, never a commit there — and
a failure there never fails the landing, nor is it retried in a loop.
The handoff states the sha it moved from and to, or why it could not
move. `workflow.md`'s *Concurrency and host rules* records the measured
reason (that checkout was **217 commits behind `origin/main`**, 26 by
first parent, and a brief quoted a `CLAUDE.md` sentence that had not
existed since 2026-10-27 because it was read there) and **why this is
the merger's own step and not a hook**: a `SubagentStop` hook matched on
`herd-merger` would work — per-agent hooks are supported and this
project already uses one, the `PreToolUse` guard in
`.claude/agents/herd-release.md` — but a hook doing it means the
session's own machinery writes the repository, which is what
`orchestrator-guard.sh` exists to prevent and what `CLAUDE.md`'s *Roles*
forbids. An agent writing the repository needs no new mechanism. I
wrote nothing suggesting `CARGO_TARGET_DIR` can be set per agent; it
stays an instruction in the brief.

### Budget evidence, both required by the brief

```
$ wc -l CLAUDE.md
150 CLAUDE.md
$ diff <(git show origin/main:CLAUDE.md | sed -n '/^## Invariants/,/^## Layout/p') \
       <(sed -n '/^## Invariants/,/^## Layout/p' CLAUDE.md)
$ echo $?
0
```

## Not done, and why

- **`docs/decisions.md` is untouched.** The brief forbade writing it
  (invariant 8: only `herd-merger` appends), so the entry is *proposed*
  below. The plan's T2 listed that file as owned; the brief overrode it.
- **`docs/orchestration/board.md` is untouched**, as required. ORCH-120
  is answered in `workflow.md` but its row still says `waiting` and
  still carries the recommendation this decision went against. Somebody
  changes that row at integration, and **ORCH-nnn (D-A) needs a row of
  its own**: I could not allocate the number.
- **The session-start duty is not in `CLAUDE.md`.** Reading the last
  `ci.yml` runs on `main` is written in `workflow.md` only, because
  `CLAUDE.md` had no line to spare (see *Risks*). That is the mitigation
  for the loss that has already happened once, so it is the first thing
  to put back if a line is ever freed.
- **No Rust gate was run and no toolchain was used.** Prose only.
- **Nothing outside my five write paths.** Four files that still encode
  the old method are named under *Risks and findings*; none was edited.

## Gates

Prose-only change, so the **four Rust gates were NOT run locally**:
`cargo build`, `cargo clippy`, `cargo fmt --check` and
`./scripts/test.sh` were not executed by this task. What covers them is
the batch's own run at integration — CI's today, since the pull request
lane is still the one in force.

```
$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
$ echo $?
0
```

`check-docs.sh` was run after every edit of this task, nine times in
all, and printed exactly that every time. The `D170` line is a gap
note, not a failure; the script exits 0 with it.

```
$ ./scripts/check-orchestration.sh | tail -2

check-orchestration: 146 passed, 0 failed
$ echo $?
0
```

**Two of the seven runs of `check-orchestration.sh` were RED, and they
are reported red.** Both failures were the same single case, on trees
whose scripts are byte-identical to `origin/main`:

```
FAIL  PATH dropped the inherited entries; the guard below needs coreutils
    /mnt/ssd_980/herd/claude/tmp/herd-orch-check.zPSHl0/stateroot/claude/tools/dtach/bin:/run/wrappers/bin:/etc/profiles/per-user/server/bin:/run/current-system/sw/bin:/wrong/home/.local/bin:/run/wrappers/bin:/wrong/home/.nix-profile/bin:/nix/profile/bin:/wrong/home/.local/state/nix/profile/bin:/etc/profiles/per-user//bin:/nix/var/nix/profiles/default/bin:/run/current-system/sw/bin

check-orchestration: 145 passed, 1 failed
```

**It is not this diff.** `git diff --stat origin/main -- scripts/ ops/
.claude/hooks/ .claude/settings.json` is empty: this task changed no
executable file at all, and two of the five green runs were made before
any edit of mine.

The case is `scripts/check-orchestration.sh:464`. It sources
`ops/agents/env.sh` under `env -i PATH="$PATH" HOME=/wrong/home` and
then asserts that the caller's **literal** `$PATH` is still a substring
of what came back. In the red runs what came back was not the caller's
PATH at all but one rebuilt from `HOME` — note `/wrong/home/.local/bin`
and `/etc/profiles/per-user//bin` with an empty user in the output
above — so `bash` in those invocations had re-derived PATH from the
environment rather than keeping the one it was given. Run by hand with
my own `$PATH`, the same inner command passes:

```
$ out=$(env -i PATH="$PATH" HOME=/wrong/home bash -c '. ops/agents/env.sh >/dev/null 2>&1; printf "%s" "$PATH"')
$ printf '%s' "$out" | grep -qF ":$PATH" && echo PRESENT
PRESENT
```

Both red runs happened in shell invocations that also wrote files; all
five green ones ran the script alone. So the verdict of that case
depends on how the invoking shell was started, not on the repository.
All seven outputs are in my scratchpad:

```
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/method-rules/out/
```

No skip: neither script has a skip mechanism and no `HERD-SKIPPED` line
appears in any of the sixteen outputs.

Whole-script timing of `check-orchestration.sh` on this host: **12 s**
before my edits, **12-13 s** after. No case was added or removed.

Both scripts were run on the tree with this handoff committed, after the
output above was written into it, so the gate is the last thing run
(ORCH-121).

**The brief expected a red `check-docs.sh` complaining about `D-NEXT`**
(gotcha `a-decision-branch-cannot-be-all-green`). It is green instead,
and consistently so: the same brief forbade writing `docs/decisions.md`,
so there is no placeholder to complain about. The gotcha does not apply
to a branch that proposes its decision in the handoff.

## Risks and findings

**Where the brief is wrong.**

1. **The `CLAUDE.md` line budget does not exist.** The brief, from the
   plan, states 8-12 lines of realistic rewrap headroom, 4 of them
   inside *Git and main*. Measured block by block over `origin/main` —
   every paragraph and bullet outside the Invariants, rewrapped at 79
   columns with its continuation indent — the theoretical gain is
   **zero**. The file is already at its optimum, so **every** line added
   must be paid for by a substitution of equal length. That is how the
   two lines it does gain were paid for: the *One PR per batch* bullet
   (4 lines) was replaced by a shorter *One landing per batch* bullet
   (3 lines), and the freed line went to the *Write only in your own
   worktree* exception. The plan's **H7 is refuted**, and the overflow
   went to `workflow.md` as the brief said it should if it came to this.
   Nothing was deleted: ORCH-001's verbatim words, the `--no-verify`
   reason, the full `gh run list` command and the session-start duty are
   all in `workflow.md`.
2. **The board row is ORCH-120, not ORCH-032.** The requirement that the
   answer and the stale clauses land in one edit is in row **ORCH-120**,
   which sits at *line 32* of `board.md` — which is where the brief's id
   came from. ORCH-032 is "Live validation V2, V3, V4, V5, V6, V10",
   done 2026-10-24. The requirement itself is real and quoted correctly.
3. **The `D-NEXT` expectation contradicts the brief's own write paths.**
   See *Gates*.
4. **"the local suite has been green" is not something I verified**, and
   the orchestrator's own second message says a full-workspace run at
   rest came back red. What I wrote is the narrower claim the record
   supports: *the suite is no longer red here for host reasons* — the
   two host-dependent causes, ORCH-069 and ORCH-107, are fixed — which
   is **not** the same as "the suite is green here". Those are different
   sentences and the difference is the whole of ORCH-nnn.
5. **One contradiction I could not remove.** The third refusal still
   says a nightly full pass is not adopted, which is true today, while
   the plan's T3 adds a `schedule:` trigger to `ci.yml`. I was told to
   mark only the second refusal adopted, so I added a sentence instead:
   if a scheduled run is added, that clause is updated in the same
   landing. If T3a lands first, the clause is briefly wrong.

**Four files outside every task's write paths still encode the old
method.** Nobody in the plan owns them:

- `docs/orchestration/permissions.md`, the file `workflow.md` points at
  for *what is technically enforced*. Line 5 (*"CI, the PR"*), line 42
  (*"branch protection makes a push to `main` fail on GitHub's side"* —
  the mechanism the method removes), line 43 (*"merge with a red check
  or an empty Evidence block … branch protection requires green `check`
  and `docs`"*) and line 44. They are **true today** and false the
  moment the protection is narrowed. This is the project's own map of
  its guarantees and it needs a task, in the same landing as the
  protection change.
- `docs/orchestration/templates/handoff.md:18`, the `**PR:**` line —
  T4 owns it. My header already says `**Landing:**`, so the two should
  land together or one of them reads oddly.
- `.claude/agents/herd-planner.md:23` names `gh pr view` as its example
  of a read-only command. Harmless, stale, not mine.
- `scripts/check-orchestration.sh:191` does the same for the
  orchestrator guard, and `:448`/`:500` carry the `GH_CONFIG_DIR`
  message *"cannot open or merge a pull request"*, shared with
  `ops/agents/env.sh` and `session.sh`. T1 owns a new section of that
  script; these lines are nobody's. The reason behind that message
  survives the method change and gets **more** important: `origin` is an
  `https://` remote whose helper is `gh auth git-credential`, so a
  session with no `gh` login cannot push at all.

**Three references in my prose point at files that do not exist yet**,
all in the same landing by the plan's order: `scripts/gate.sh` (T1),
`scripts/githooks/pre-push` (T5) and `templates/landing.md` (T4). Each
is now guarded by tense — the gate bullet is marked proposed, the hook
line in step 4 says *"once `scripts/githooks/pre-push` exists"* — but a
reader still meets three names with nothing behind them if any of the
three slips. `check-docs.sh` does not catch this: it asserts only the
three paths `CLAUDE.md` names. **T5 is the one to watch**, since the
plan allows it to be deferred while *Unchanged* and the cost list both
lean on that hook.

**The flaky gate case is a finding in its own right.** Under the
proposed method `check-orchestration.sh` is part of the only gate a
landing gets, and one of its 146 cases returns a verdict that depends on
the invoking shell. See *Gates*; it wants a board row.

**What I chose, and what I rejected.**

- I kept the section's history rather than rewriting it: both quotes,
  both dated, the old section title named so that older rows still find
  it, and commit 4 left in the log although commit 7 undid most of it.
  Rejected: a clean page with only the current rule.
- I marked the refusal's two errors **inside** the quotation rather than
  correcting the quote silently or leaving a false sentence beside a
  true one. Rejected: dropping the clause, which would have hidden that
  the number was ever wrong, and paraphrasing, since it is Alexa's
  refusal.
- I wrote the losses into `workflow.md` and not only here, including the
  reduction in Alexa's oversight and the fact that *"GitHub is just
  storage"* is not literally true. Rejected: a softer summary.
- I moved the gate to **after** the merger's own board and status
  commits. Rejected: the plan's step order, which produced evidence for
  a tree that is not the one pushed — ORCH-121's finding, applied to the
  merger itself.
- I wrote the two corrected measurements in the narrow form I was given
  and nowhere wrote the dramatic version. Rejected: keeping the plan's
  §5 sentence, which is false for an agent's session.
- I put the lane split in the agent files rather than only in
  `workflow.md`. Rejected: leaving `herd-merger.md` describing a lane
  that is not in force, which is how an agent ends up pushing `main`.

## Decision to record (proposed, for `herd-merger` to allocate)

Not written into `docs/decisions.md` by this branch. **Two entries, not
one**, because one half is adopted and the other is pending — and the
pending half is the one the board row ORCH-nnn tracks.

> `## 2026-10-28 — D-NEXT The pull request leaves the loop: one landing
> per batch`
>
> **Decision (Alexa, 2026-10-28).** Verbatim: *"en gros je verais juste
> la supresions des pr et sortir de l'env Github … un agent peux tester
> en local et le merge aussi avant d'envoyer vers main, Github est juste
> un centre de stokage … (regles aussi les problemme des 2000 minute de
> run)"*. The pull request leaves the loop. A landing is one batch, one
> push, one `--no-ff` merge commit whose message is the landing record,
> one post-push run of `ci.yml` on `main` — one-to-one, because that
> chain is what keeps a red run attributable. Six runs on `main` were
> `cancelled` in the five days before, three within seven seconds of
> each other.
>
> **What it answers.** ORCH-120, against the recommendation in that row
> (*"not yet — keep CI as the gate"*) and wider than its fallback. The
> refusal of 2026-10-27 made itself reconsiderable once ORCH-069 and
> ORCH-107 were fixed; both landed, so this is that sequence arriving,
> not an override.
>
> **Nothing waits for a review.** Asked whether rule changes
> (`CLAUDE.md`, the invariants, `.claude/`, a release cut) should keep a
> pull request so he could look before they land, Alexa answered no:
> everything lands locally, rules included, and he reads `main` when it
> suits him. This is a reduction in oversight and is recorded as one.
>
> **What this does not decide.** The merge condition, which is the
> entry below and is pending. The branch protection change (Alexa's act
> on the GitHub settings), the `ci.yml` triggers, `gate.sh`'s contents,
> the `pre-push` hook, and whether a nightly run is adopted.
> `release.yml` and `release-dev.yml` are untouched: GitHub remains the
> release factory (D162, D168), the hotfix lane (D172), the force-push
> and deletion protection on `main`, the push credential and our only
> second machine.
>
> **Reversibility.** High on our side, low on GitHub's.

> `## 2026-10-28 — D-NEXT What a green local gate proves: pending`
>
> **Proposed and not in force.** The merge condition becomes a green
> `./scripts/gate.sh` for the exact sha pushed plus a filled landing
> record. **Alexa re-granted ORCH-001 on these terms on 2026-10-28** and
> his grant is not what is in doubt. What is in doubt is what a green
> local run proves, measured the same day:
>
> - A full-workspace run at rest on this workstation came back **red**:
>   three `real_herd` 30 s timeouts that **pass when their targets are
>   run alone**. The suite self-inflicts enough contention to break its
>   own waits.
> - The gate measures a **cgroup slice**, not a host: the runner
>   announced 4.9 GiB rather than 31.1 GiB because `/chatons.slice`'s
>   `memory.high` is 18 GiB and already held 13 GiB. Two agents in
>   different slices do not run the same suite.
> - **Neither host** runs the twelve confinement assertions behind
>   invariant 2: all twelve skips here are the same cgroup-delegation
>   refusal. This host runs **two** assertions CI cannot, which is the
>   whole of the 12-against-14 difference.
>
> **Until this is answered**, `CLAUDE.md`'s rule stands unchanged: one
> pull request per batch, CI green on `check` and `docs`, Evidence
> filled, merged by `herd-merger` under ORCH-001.

## Decisions needed

1. **ORCH-nnn (D-A): what a green local gate proves**, which is the
   second entry above and the thing the whole method waits on.
   *Recommendation:* mine is not worth much without a measurement —
   settle the self-inflicted contention of a full-workspace run first,
   since a gate that is red at rest cannot be a merge condition whatever
   anyone decides. *Fallback:* the pull request lane stays, which costs
   nine minutes a batch and is what we pay today.
2. **The wording of the re-granted ORCH-001** (the plan's D-2). The
   condition is strictly weaker than the one Alexa agreed to on
   2026-10-24. It is now written as *proposed* in `workflow.md` and
   nowhere as in force. *Recommendation:* leave it proposed until 1 is
   answered. *Fallback:* he corrects the sentence when he reads `main`.
3. **Who fixes `docs/orchestration/permissions.md`.**
   *Recommendation:* one `herd-step` branch in the landing that carries
   the protection change, because that file is the project's claim about
   what a machine still refuses. *Fallback:* a board row with a date,
   and the file is known-wrong until then.
4. **A row for the flaky `check-orchestration.sh:464`**, red twice in
   seven runs on an unchanged tree. *Recommendation:* open it — the case
   asserts the invoking shell's literal `$PATH` and this script is part
   of the only gate the proposed method gets. *Fallback:* nothing, and
   the next agent spends the time I spent proving it was not its diff.
5. **Whether ORCH-120's board row is closed by `herd-merger` at
   integration or by a separate task.** *Recommendation:* the merger,
   which already owns `board.md`.

## Resume point

`herd-reviewer` on this branch, as `workflow.md` step 6 requires and as
the brief calls non-negotiable. The review's first step is the
Invariants diff in *Done*, then the tense of every sentence about the
gate (commit 7 is where that was corrected, and a sentence that still
makes a green local gate the merge condition is a bug), then the three
names with nothing behind them, then `herd-merger.md` steps 5 to 9.
Nothing else in this task is pending.

## Processes

No process started, no PID, no background job, no compile, no `cargo`.
No `target/` was created, so none was deleted. The only writes were in
my own worktree and my own scratchpad subdirectory:

```
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/method-rules/wt
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/method-rules/out
```

The orchestrator's checkout was read with `git -C … fetch` once, to
create this worktree, and never written.
