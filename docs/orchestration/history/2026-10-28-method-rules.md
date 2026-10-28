# method-rules — handoff 2026-10-28

- **Item:** ORCH-120 (answered 2026-10-28), with the two stale clauses
  ORCH-120's own row requires in the same edit
- **Agent:** herd-step
- **Branch / commit:** `method-rules` / see below, based on origin/main
  `bc83717`
- **Landing:** none — this branch is pushed and opens no pull request;
  it lands with its batch, and `herd-merger` names the landing here
- **Task:** T2 of the local-merge method (plan of 2026-10-28 by
  `herd-planner`, in that agent's scratchpad). Prose only, no code.

## Done

Five commits, one idea each, all on `method-rules`.

1. **Replace one PR per batch with one landing per batch**
   (`docs/orchestration/workflow.md`). The section *One PR per batch of
   branches* becomes *One landing per batch* and carries Alexa's words
   of 2026-10-28 verbatim, the statement that this answers **ORCH-120**
   against the recommendation in that row and wider than its fallback,
   and a new subsection *What a landing is*: one batch, one push, one
   `--no-ff` merge commit whose message is the landing record, one
   post-push run of `ci.yml` on `main`. The loop's step 4, step 7 and
   step 8, the roles table and *Permissions and escalation* follow. The
   transition is written where nobody can miss it: GitHub still requires
   a pull request on `main` today, so the method cannot be used until
   the branch protection is narrowed, and this change itself lands
   through one last pull request.
2. **Mark the local gate adopted and fix two stale clauses.** The
   refusal of 2026-10-27 is marked **ADOPTED 2026-10-28**, not deleted;
   its original reasoning is quoted as written with its two factual
   errors marked in place (*"the suite is red here for host reasons"* —
   no longer true; *"12 here against 15 on CI"* — the 15 is wrong), and
   the corrected figures follow: 12 here at rest, 14 here under load,
   14 on CI (ORCH-116, run 40538722315), the 15 coming from the *"AT
   LEAST 15"* of red run 40439537929, which is what D179 labels. The
   other two refusals keep their date and their reason.
3. **Write down what the local merge costs** — seven items in
   `workflow.md`, not in a handoff nobody re-reads, plus a procedure
   for a red post-push run on `main`.
4. **Rewrite Git and main for the local landing** (`CLAUDE.md`), with
   ORCH-001 re-granted on the two new conditions and *nothing waits for
   Alexa* stated as the reduction in oversight it is. Session start now
   reads the last `ci.yml` runs on `main`.
5. **Update the three agent roles** (`herd-merger`, `herd-step`,
   `herd-release`), and a sixth small commit ties the nightly refusal to
   the workflow file that would end it.

`herd-merger` changed most: `gate.sh` is its only gate and is now
**step 5, after its own board and status commits**, where it used to be
step 4, before them — the old order gated a tree that was not the one
pushed, which is ORCH-121's finding applied to the merger itself. The
landing record is written before the merge commit it becomes, the merge
is `--no-ff`, the push is once per batch, and a new step 8 records the
post-push run id without waiting for its verdict.

Budget evidence, both required by the brief:

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
  (invariant 8: only `herd-merger` appends), so the entry this change
  needs is *proposed* below instead. The plan's T2 listed that file as
  owned; the brief overrode it and the brief is what I followed.
- **`docs/orchestration/board.md` is untouched**, as the brief required.
  ORCH-120 is answered in `workflow.md`; its board row still says
  `waiting` and still carries the recommendation this decision went
  against. Somebody has to change that row at integration.
- **No Rust gate was run and no toolchain was used.** Prose only.
- **Nothing outside my five write paths.** Four files that encode the
  old method are named under *Risks and findings*; none was edited.

## Gates

Prose-only change, so the **four Rust gates were NOT run locally**:
`cargo build`, `cargo clippy`, `cargo fmt --check` and
`./scripts/test.sh` were not executed by this task, and after this
change there is no CI on a pull request to run them either — the
landing's own `./scripts/gate.sh` is what covers them, and that is
`herd-merger`'s run, not mine.

```
$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
$ echo $?
0
```

```
$ ./scripts/check-orchestration.sh | tail -3
ok    ops/agents names paths only, never a token (invariant 7)

check-orchestration: 146 passed, 0 failed
$ echo $?
0
```

Whole-script timing of `check-orchestration.sh` on this host: **12 s**
before my edits and **12-13 s** after, measured with `date +%s` around
the run. No case was added or removed by this task.

**One of the five runs of `check-orchestration.sh` was RED, and it is
reported red.** Run 3 of 5, the one made right after this handoff was
first committed:

```
FAIL  PATH dropped the inherited entries; the guard below needs coreutils
    /mnt/ssd_980/herd/claude/tmp/herd-orch-check.zPSHl0/stateroot/claude/tools/dtach/bin:/run/wrappers/bin:/etc/profiles/per-user/server/bin:/run/current-system/sw/bin:/wrong/home/.local/bin:/run/wrappers/bin:/wrong/home/.nix-profile/bin:/nix/profile/bin:/wrong/home/.local/state/nix/profile/bin:/etc/profiles/per-user//bin:/nix/var/nix/profiles/default/bin:/run/current-system/sw/bin

check-orchestration: 145 passed, 1 failed
```

Runs 1, 2, 4 and 5 were `146 passed, 0 failed`, two of them before any
edit of mine and two after that red one, on the same commit. **It is not
this diff:** `git diff --stat origin/main -- scripts/ ops/
.claude/hooks/ .claude/settings.json` is empty, and this task changed no
executable file at all.

The case is `scripts/check-orchestration.sh:464`. It sources
`ops/agents/env.sh` under `env -i PATH="$PATH" HOME=/wrong/home` and
then asserts that the caller's **literal** `$PATH` is still a substring
of what came back. In the red run what came back was not the caller's
PATH at all but a NixOS-default one rebuilt from `HOME` — note
`/wrong/home/.local/bin` and `/etc/profiles/per-user//bin` with an empty
user in the output above. So the case is keyed on how the shell that
invokes it was started, not on anything in the repository, and it is a
flake with a known mechanism rather than a mystery. Four full outputs
are in my scratchpad at
`…/scratchpad/method-rules/out/final-orch*.txt`. **Recommendation: a
board row** — a gate case that depends on the invoking shell's `PATH`
will fail again, and under the new method this script is part of the
only gate a landing gets.

No skip: neither script has a skip mechanism, and no `HERD-SKIPPED`
line exists in either output. The `D170` note is a gap note, not a
failure; `check-docs.sh` exits 0 with it.

**The brief expected a red `check-docs.sh` complaining about `D-NEXT`**
(gotcha `a-decision-branch-cannot-be-all-green`). It is green instead,
and that is consistent: the same brief forbade writing
`docs/decisions.md`, so there is no placeholder to complain about. The
gotcha does not apply to a branch that proposes its decision in the
handoff.

Both scripts were run on the tree with this handoff committed, after the
output above was written into it, so the gate is the last thing run
(ORCH-121). `check-docs.sh` printed exactly what is quoted, every time.

## Risks and findings

**Where the brief is wrong.**

1. **The line budget does not exist.** The brief, from the plan, states
   8-12 lines of realistic rewrap headroom in `CLAUDE.md`, 4 of them
   inside *Git and main*. Measured block by block over the real file —
   every paragraph and bullet outside the Invariants, rewrapped at 79
   columns, continuation indent included — the theoretical gain is
   **1 line**, and that one line is inside the section I was rewriting.
   The file is already wrapped at its optimum. So the plan's H7 is
   **refuted**, and the new rules were paid for the way the brief said
   to pay for them if it came to this: the detail moved to
   `workflow.md`. What moved, and nothing was deleted: ORCH-001's
   verbatim *"non il peux merge par defaut"* (now in *Permissions and
   escalation*), the reason `--no-verify` matters more now (in
   *Unchanged*), and the full `gh run list --workflow ci.yml --branch
   main --limit 5` (in *What a landing is*). `CLAUDE.md` is 150/150 and
   the Invariants section is byte-identical to `origin/main`.
2. **The board row is ORCH-120, not ORCH-032.** The requirement that
   the answer and the stale clauses land together is in row **ORCH-120**
   — which sits at *line 32* of `board.md`, which is where the brief's
   id came from. ORCH-032 is "Live validation V2, V3, V4, V5, V6, V10",
   done 2026-10-24. The requirement itself is real and quoted correctly.
3. **The `D-NEXT` expectation contradicts the brief's own write paths.**
   See *Gates*.
4. **"the local suite has been green" is not something I verified.** No
   toolchain ran here. The corrected clause rests on ORCH-069 and
   ORCH-107 being `done` on `origin/main` and on ORCH-107's measured
   four runs under induced load with zero `:649`/`:789` timeouts — not
   on a run of my own. If that is not enough for a sentence in
   `workflow.md`, the sentence is the one to revisit.

**Four files outside every task's write paths still encode the old
method.** Nobody in the plan owns them, and they are not cosmetic:

- `docs/orchestration/permissions.md`, the file `workflow.md` points at
  for *what is technically enforced*. Line 5 (*"CI, the PR"*), line 42
  (*"branch protection makes a push to `main` fail on GitHub's side"* —
  the mechanism we are deliberately removing), line 43 (*"merge with a
  red check or an empty Evidence block … branch protection requires
  green `check` and `docs`"*) and line 44. **After T0 these are false,
  and they are the project's own map of its guarantees.** This needs a
  task; I recommend one branch, in the same landing as T0.
- `docs/orchestration/templates/handoff.md:18`, the `**PR:**` line —
  T4 owns it. My own header says `**Landing:**` already, which is T4's
  wording, so the two must land together or one of them reads oddly.
- `.claude/agents/herd-planner.md:23` names `gh pr view` as its example
  of a read-only command. Harmless, stale, not mine.
- `scripts/check-orchestration.sh:191` does the same for the
  orchestrator guard, and `:448`/`:500` carry the `GH_CONFIG_DIR`
  message *"cannot open or merge a pull request"*, shared with
  `ops/agents/env.sh` and `session.sh`. T1 owns a new section of that
  script; these existing lines are nobody's.

**Three references in my prose point at files that do not exist yet**,
all of them in the same landing by the plan's order: `scripts/gate.sh`
(T1), `scripts/githooks/pre-push` (T5) and
`docs/orchestration/templates/landing.md` (T4). If any of the three
slips to a later landing, my text promises a file that is not there.
`check-docs.sh` does not catch this: it only asserts the three paths
`CLAUDE.md` names. **T5 is the one to watch**, since the plan allows it
to be deferred and `workflow.md` now tells every agent to run
`git config core.hooksPath scripts/githooks` in a new worktree — which
is harmless with no hook directory, but the *Unchanged* paragraph and
the cost list both lean on that hook being the only mechanism in front
of `main`.

**One contradiction I could not remove.** The third refusal still says a
nightly full pass is not adopted, which is true today, while the plan's
T3 adds a `schedule:` trigger to `ci.yml`. I was told to mark only the
second refusal adopted, so I added one sentence instead: if a scheduled
run is added, that clause is updated in the same landing. If T3a lands
first, the clause is briefly wrong and the reviewer should catch it.

**What I chose, and what I rejected.**

- I kept the section's history rather than rewriting it: the 2026-10-27
  quote, the 2026-10-28 quote, both dated, and the old section title
  named so that board rows and handoffs pointing at *One PR per batch of
  branches* still find it. Rejected: a clean page with only the current
  rule.
- I marked the refusal's two errors **inside** the quotation rather than
  silently correcting the quote or leaving a false sentence beside a
  true one. Rejected: dropping the clause (it would have hidden that the
  number was ever wrong) and paraphrasing (it is Alexa's refusal).
- I wrote the losses into `workflow.md` and not only here, including the
  one that is a reduction in Alexa's oversight and the one that says
  *"GitHub is just storage"* is not literally true. Rejected: a softer
  summary; the brief asked for the honest version and the honest version
  is what makes the `release.yml` sentence load-bearing.
- I moved `gate.sh` after the merger's board commits. Rejected: keeping
  the plan's step order, which would have produced a receipt for a sha
  that is not the one pushed — the exact failure the receipt exists to
  prevent.

## Decision to record (proposed, for `herd-merger` to allocate)

Not written into `docs/decisions.md` by this branch. Header and body as
I would have them, `D-NEXT` to be replaced with the next free number
(D185 at the time of writing):

> `## 2026-10-28 — D-NEXT The pull request goes away: one landing per
> batch, gated locally`
>
> **Decision (Alexa, 2026-10-28).** Verbatim: *"en gros je verais juste
> la supresions des pr et sortir de l'env Github … un agent peux tester
> en local et le merge aussi avant d'envoyer vers main, Github est juste
> un centre de stokage … (regles aussi les problemme des 2000 minute de
> run)"*. The pull request is removed from the loop entirely. A landing
> is one batch, one push, one `--no-ff` merge commit whose message is
> the landing record, one post-push run of `ci.yml` on `main`.
>
> **What it answers.** ORCH-120, against the recommendation in that row
> (*"not yet — keep CI as the gate"*) and wider than its fallback. The
> refusal of 2026-10-27 made itself reconsiderable once ORCH-069 and
> ORCH-107 were fixed; both landed, and this is the sequence arriving,
> not an override.
>
> **ORCH-001 re-granted, not reinterpreted.** Both halves of the
> 2026-10-24 condition (CI green, Evidence block filled) no longer
> exist. `herd-merger` alone pushes `main`, on a green
> `./scripts/gate.sh` for the exact sha being pushed plus a filled
> landing record. Pushing a tag and publishing a release are unchanged:
> Alexa's word, one act at a time.
>
> **Nothing waits for a review.** Asked whether rule changes
> (`CLAUDE.md`, the invariants, `.claude/`, a release cut) should keep a
> pull request so he could look before they land, Alexa answered no:
> everything lands locally, rules included, and he reads `main` when it
> suits him. This is a reduction in oversight and is recorded as one.
>
> **What this costs.** Seven items in `workflow.md`, *What the local
> merge costs*. The two that are structural: a machine's refusal becomes
> an agent's judgement, and a post-push run on `main` has never once
> been read (run 40439537929, red, after PR #91, in neither `board.md`
> nor `status.md`).
>
> **What this does not decide.** The branch protection change itself
> (Alexa's act on the GitHub settings), the `ci.yml` triggers, the
> `gate.sh` contents, the `pre-push` hook, and whether a nightly run is
> adopted. `release.yml` and `release-dev.yml` are untouched: GitHub
> remains the release factory (D162, D168), the hotfix lane (D172), the
> force-push protection and our only second machine.
>
> **Reversibility.** High on our side, low on GitHub's: the text and the
> hook are a landing away, but re-requiring a pull request means putting
> the branch protection back, which is Alexa's act again.

## Decisions needed

1. **The wording of the re-granted ORCH-001**, as the plan's D-2 asked.
   The condition is strictly weaker than the one Alexa agreed to on
   2026-10-24, and it is now written in `CLAUDE.md`, `workflow.md` and
   `herd-merger.md`. *Recommendation:* let it stand — his words of today
   cover it. *Fallback:* he corrects the sentence when he reads `main`,
   and a later decision records the correction.
2. **Who fixes `docs/orchestration/permissions.md`.** *Recommendation:*
   one `herd-step` branch in the landing that carries T0, because that
   file is the project's claim about what a machine still refuses.
   *Fallback:* a board row with a date, and the file is known-wrong
   until then.
3. **Whether the board row ORCH-120 is closed by `herd-merger` at
   integration or by a separate task.** *Recommendation:* the merger,
   since it already owns `board.md`.
4. **A row for the flaky `check-orchestration.sh:464`**, which went red
   once in five runs on an unchanged tree (see *Gates*).
   *Recommendation:* open it — the case asserts the invoking shell's
   literal `$PATH`, and this script is part of the only gate a landing
   will get. *Fallback:* nothing, and the next agent spends the time I
   spent working out that it was not its diff.

## Resume point

`herd-reviewer` on this branch, as `workflow.md` step 6 requires and as
the brief says is non-negotiable. The review's first step is the
Invariants diff above, then the three dangling references and the
ordering change in `herd-merger.md` steps 4 and 5. Nothing else in this
task is pending.

## Processes

No process started, no PID, no background job, no compile. No `target/`
was created, so none was deleted; the only writes were in the worktree
`…/scratchpad/method-rules/wt` and in
`…/scratchpad/method-rules/out`.
