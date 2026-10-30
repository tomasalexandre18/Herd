# Orchestration workflow

How a request becomes merged work, who may do what, and what every agent
owes before it stops. `CLAUDE.md` is the contract; this is the procedure.

## Roles and rights

| Role | Does | Never |
|---|---|---|
| orchestrator (main session) | receives the goal, reads the board, picks agents, writes briefs, reads evidence, asks Alexa one question at a time, reports in French | edit a repository file, commit, merge, resolve a conflict, tag, release, declare a skip a pass, report an agent's claim unverified |
| `herd-planner` | analyses a request, lists alternatives and hypotheses to refute, cuts bounded tasks | modify the product, merge, settle an owner decision |
| `herd-step` | one bounded task on its own branch and worktree, with the gates the task needs, evidence and a handoff; pushes the branch | open a pull request, leave its write paths, touch `main`, touch another agent's worktree, or the running instance |
| `herd-merger` | integrates a batch of branches, one at a time onto one integration branch: conflicts, the local gate, D-number allocation, board and status update, then the landing record and **one** `--no-ff` merge commit on `main` with that record as its message, pushed once under standing authorisation ORCH-001, then pushes that release's annotated tag (D212) | open a pull request — but the one that lands this change, on the orchestrator's instruction (*Not an option*, step 1) — bypass a gate, push `main` without a green gate for the merge's second parent, decide a specification or invariant change, operate Herd |
| `herd-release` | prepares a release: checklist, version, body, evidence; simulation only until Alexa orders a release | tag, push a tag, publish, deploy, land its own branch |
| `herd-reviewer` | adversarial review of a plan, a rule change, a permission change or an unresolvable conflict; lists missing evidence and adverse cases | edit anything, replace the owner |

What is technically enforced and what is only written is in
`permissions.md`.

## The loop

1. **Board first.** The orchestrator reads `board.md`; a request that is
   already an item resumes it, a new one gets an `ORCH-nnn` id.
2. **Plan when structuring.** A milestone, a rule change or a design
   choice goes to `herd-planner`; a one-file fix does not.
3. **Brief.** One agent, one bounded task, from `templates/brief.md`.
   The brief names the base commit, the worktree, the write paths, the
   files to read in order, the gates to run and the stop condition.
4. **Execute.** `herd-step` works in `.claude/worktrees/<task>/` or a
   scratchpad subdirectory on branch `<task>` and ends with a handoff
   from `templates/handoff.md` saved as
   `docs/orchestration/history/<date>-<task>.md` on that branch. **The
   docs gate is the last thing run** — after the handoff is committed,
   before the push: a handoff is itself a docs file, so a gate run
   before it was written tested a different tree, and under one landing
   per batch one ungated handoff reddens the whole batch's gate run,
   where it blocks every other branch in it. **That last run's output
   therefore cannot be in the handoff** — the tree it gated is the tree
   that is pushed, and nothing may be added to it afterwards — so the
   handoff's *Gates* section **names** that run and where its output
   went, the verbatim output goes to the agent's report, and the output
   quoted in the handoff is from the runs made before the commit.
   `templates/handoff.md` carries the shape, which is the one
   `.claude/agents/herd-merger.md` step 6 already uses. **Then** the
   branch is pushed. **Nobody opens a
   pull request** (*One landing per batch*): the branch and the handoff
   are the delivery. An agent sets no git hook of its own:
   `core.hooksPath` is **one repository-wide setting**, not a
   per-worktree one, and it is *Concurrency and host rules*' business.

   **Anything the merger must act on is in the handoff, in full.** A
   drafted `docs/decisions.md` entry — headed `D-NEXT`, because step 7
   allocates the number — and a drafted `board.md` row are written out
   verbatim in the handoff's *Drafted for `herd-merger`* section, never
   as a pointer to the agent's report and never as a summary. **The
   merger reads the tree and nothing else**: an agent's hand-back report
   goes to the orchestrator and the orchestrator's conversation is not
   an artefact, so a draft that lives only there does not reach step 7.
   Measured 2026-10-29: `spool-offer-fix` deferred its entry to *"the
   agent's report"* and D221 had to be composed from that handoff's
   *Done* section, which `history/2026-10-29-merge-batch-10.md` records
   as not the author's wording. It was faithful; the method had not made
   it so.
5. **Verify.** The orchestrator reads the diff, the gate output and the
   handoff. It reruns a read-only check when a claim matters. It never
   fixes what it finds; it sends it back or opens a new task.
6. **Review when it is a rule.** A change to `CLAUDE.md`, agents,
   permissions, hooks, `check-docs.sh` or an invariant gets a
   `herd-reviewer` pass before integration; a micro-edit does not.
7. **Integrate and land.** `herd-merger` takes a **batch** of finished
   branches — everything ready when it starts — and merges them **one at
   a time** onto one integration branch off `origin/main`: conflicts
   resolved in its worktree, `D` numbers allocated once in order
   (`D-NEXT` → next free number, gaps stay), `board.md` and
   `docs/status.md` written once, and the landing record written from
   `templates/landing.md`, **then its own handoff, committed — the last
   change to the tree, because the gate has to run on what is pushed**
   (ORCH-121 applied to the merger itself). It then runs
   `./scripts/gate.sh` on that branch, merges it `--no-ff` onto `main`
   in its own worktree with the record as the merge commit message, and
   pushes `main` **once**: Alexa's standing authorisation (ORCH-001,
   2026-10-24, re-granted 2026-10-28) covers any landing whose gate is
   green on **the sha that was gated — the merge's second parent, with
   an identical tree** — and whose landing record is filled. `main`
   moves no other way, and no pull request is opened. A tag is not part
   of this step: it is its own act, after the push, and step 8 holds it.
   The rule, what a landing is, what it costs, the transition and the
   three method changes refused with it: *One landing per batch*, below.
8. **Release.** On Alexa's order, `herd-release` prepares the checklist
   and pushes the `cut-v<version>` branch that bumps the version;
   `herd-merger` lands it, as for any other branch, and **then pushes
   the annotated tag itself** (Alexa, 2026-10-29, **D212**). The
   integrator holds the tag on the conditions it already lands under — a
   green gate for the gated sha, a filled record — and tags the landing
   commit by name, the only commit `release.yml`'s version check accepts.
   One named owner, in place of an act nobody held. Nothing in this
   project deploys. **A release's own notes stay Alexa's own act, and a
   brief says so from the start** (**D210**, 2026-10-29): `release.yml`
   never reads the prepared human half, and once a release is public
   neither an execution agent nor the orchestrator can put it there —
   two permission walls, neither a defect, measured at v0.4.0. Until the
   notes ship in the tree (ORCH-162) the paste is Alexa's own, named in
   the same order as the tag.

## One landing per batch

The rule of 2026-10-27 and the rule of 2026-10-28, in that order,
because the second grew out of the first. Until 2026-10-28 this section
was titled *One PR per batch of branches*; board rows and handoffs
written before that date point at the old name.

Alexa, 2026-10-27, verbatim: *"pour le moment on applique uniquement le
une PR = plusieur lot, le reste non"* — answering a proposal modelled on
how the first consumer works. One part of that proposal was adopted;
three were refused, and the refusals are written down below with their
reason so that nobody re-adopts them by drift. One of the three was
adopted the next day, which is why each keeps its date.

Alexa, 2026-10-28, verbatim: *"en gros je verais juste la supresions des
pr et sortir de l'env Github, comme chatOnS il font tout les test en
local et c'est merger en local et pousser, sa part du principe que il y
a pas de colaboratif distant ce qui est le cas, seulement vous sur le
project, sa evite les longue CI en boucle, un agent peux tester en local
et le merge aussi avant d'envoyer vers main, Github est juste un centre
de stokage, c'est une revisite de la facons de travailler (regles aussi
les problemme des 2000 minute de run)"*

That is the answer to **ORCH-120**, dated 2026-10-28. It is given
against the recommendation written in that row (*"not yet — keep CI as
the gate"*) and it is wider than the row's fallback, which would have
kept CI and changed only what the merger reads. It also answers a
question ORCH-120 never asked: the pull request itself goes.

**The pull request goes away.** Nobody opens one — not an agent, not
`herd-merger`, not for a rule change, not for a release cut — **but the
one that lands this change**, which `herd-merger` opens on the
orchestrator's instruction, once, dated 2026-10-28 (*Not an option*,
step 1). There is no
remote collaborator: Alexa and this project's agents are the whole set
of authors, so a pull request bought review from nobody and cost about
nine minutes of CI per batch on the critical path (PR run 40562916742,
8 m 54 s, 2026-10-27).

### Not an option: the obligation, and the transition to it

The objection was raised — a local gate is one machine, and on
2026-10-28 this one could not produce a green run at all — and Alexa
answered it. Verbatim, 2026-10-28: *"que l'on soit clair, l'abandon des
PR et CI Github n'est pas une option, mais une obligation."* So the
method above **is the rule**, `CLAUDE.md` carries it, and the red suite
is a problem to solve rather than a reason to wait. What follows is a
dated transition, not a condition in the contract: when the last step is
done this subsection goes, and nothing above it changes.

**Three steps, so that nobody improvises the order. There are three and
not four because the obvious fourth cannot be done in its own step:**

1. **One last pull request carries the whole method change**, so that
   the method is verified by the thing it replaces before that thing is
   removed. **`herd-merger` opens it** — the one exception to *nobody
   opens a pull request*, dated 2026-10-28, for this landing only and
   only when the orchestrator instructs it. It is the only actor that
   can: `orchestrator-guard.sh` denies the main session
   `gh pr create`, measured 2026-10-28. Its body is the landing record,
   filled from `templates/landing.md` as for any landing. It carries the
   deletion of `.github/pull_request_template.md` **and nothing else of
   `.github/`** — see the note after step 3.
2. **The branch protection on `main` is narrowed:**
   `required_pull_request_reviews` and `required_status_checks` dropped,
   `allow_force_pushes: false` and `allow_deletions: false` **kept**.
   Alexa has authorised an agent to do it; it is a separate task and it
   is not in this one. **It is a tidying step and not a safety one:**
   `enforce_admins` is false and our only push credential is a
   repository admin, so GitHub does not in fact refuse a direct push to
   `main` today (*What the local merge costs* has the measurement).
   Nothing but step 3's own preconditions keeps the order.
3. **The first local landing**, which is also the first use of the
   method on something other than itself, and which **carries the
   deletion of `ci.yml`'s `pull_request:` trigger** (see below). **Six
   things must be true before it**, none of them a pull request, and the
   two that get forgotten are the fifth and the sixth:
   `scripts/gate.sh` is in the
   tree, so that the condition in `CLAUDE.md` can be met at all;
   `docs/orchestration/templates/landing.md` is in the tree, so that the
   record has a shape; `scripts/githooks/pre-push` is in the tree;
   `core.hooksPath` is set on the repository (*Concurrency and host
   rules*); **the test fix is on `main`** — branch
   `real-herd-patience-timeouts`, landed in the product batch of
   2026-10-28; and **this host's expected skip lists are in the tree**,
   `scripts/expected-skips/local.txt` and
   `scripts/expected-skips/local-under-load.txt`. That fifth one is not
   only about the first landing:
   **`gate.sh` runs the full suite**, so until that fix is on `main`
   every `gate.sh` is red and **no landing at all is possible**, this
   one or any later one. The first three are on pushed branches of the
   same batch as this file and are **not on `main` yet** — this file and
   they land together or the contract names three things that do not
   exist. Without the hook and
   the hooks path there is **no mechanism at all** in front of `main`,
   because the branch protection never bound our own credential, and
   the *Unchanged* paragraph above would be promising a guarantee that
   was never created.

   **The sixth is the one that actually blocked everything on the
   afternoon of 2026-10-28**, and it is not a red gate but no gate:
   without `scripts/expected-skips/local.txt` the gate **refuses** — no
   verdict, exit 3, *"produce one with `--bless` after a green suite"* —
   so there is nothing a landing could rest on. The second file,
   `local-under-load.txt`, is a **delta over the at-rest list and never a
   full list**: the two load-gated flood benches and nothing else
   (`scripts/expected-skips/README.md` documents it that way). The
   distinction is load-bearing. Blessing a loaded run straight into
   `local.txt` would put all fourteen names there, and an at-rest run
   would then be green with a flood bench silently skipped — an
   assertion nobody checks, reported as a pass, which is the one thing
   D27 exists to forbid.

**Why the `pull_request:` trigger cannot go in step 1, which is the
whole reason there are three steps and not four.** `ci.yml` triggers on
`pull_request:` and on `push: main`. A pull request that deletes its own
`pull_request:` trigger produces **no `check` and no `docs`** — the
workflow file is read from the merge ref — so the two required contexts
never report and that pull request can never go green or be merged.
Deleting the trigger is therefore only possible in a lane that needs no
pull request, which is the first local landing itself. `release.yml` and
`release-dev.yml` are **never** deleted, in any step: a release needs
them (D162), and *What the local merge costs* says what else GitHub
keeps doing for us.

**What step 3 no longer waits on, because it was fixed.** Until
2026-10-28 a full-workspace run at rest on this workstation failed
reproducibly — three times out of three — on three `real_herd` 30 s
timeouts: `a_result_the_hub_cannot_write_is_kept_until_it_can`,
`a_drain_first_reservation_survives_a_kill_nine_on_either_side` and
`a_metal_job_drains_the_build_slot_first_and_no_build_overlaps_it`. It
was a **test** defect, not a product one, and the hub had been printing
the reason in words all along: the runner announced **one job slot**,
because D119's cgroup clamp cut the announced room to 4.6 GiB, and each
of those three fixtures silently needed two concurrent jobs. One
declared constant in the three tests, `--job-memory-max=512MiB`, fixed
it. **The full suite is now green at rest: 0 failed, 12 skips, `test
suite: PASSED`** — measured on `ee8dec0`, the head of branch
`real-herd-patience-timeouts`, `test.sh exit=0 wall=170s`, 55
`test result:` lines and none of them FAILED, with the twelve skips
named one by one in
`history/2026-10-28-real-herd-patience-timeouts.md`. The sha is here
because this is the sentence the whole method rests on, and it was the
last durable claim in this file with nothing behind it: a method whose
gate had never once been green would be worth doubting, and this is the
run that says it has been.

### What a landing is

**One batch, one push, one `--no-ff` merge commit whose message is the
landing record, one post-push run of `ci.yml` on `main`.** That chain is
one-to-one on purpose, and it is the whole of what keeps a failure
attributable: the run that goes red names a sha, the sha is a merge
commit, the merge commit names the batch and each of its branches. Break
the chain — two pushes for one batch, or a fast-forward with no merge
commit — and a red run points at a blur with nothing to bisect over.
That is not hypothetical: `ci.yml`'s concurrency group cancels an
in-progress run on `main` when a second push arrives, and **six runs on
`main` were cancelled in the five days to 2026-10-28**, three of them
within seven seconds of each other. Each cancelled run was the only
thing a second machine would ever have said about its own landing.

- One branch per task is unchanged. An agent finishes by **pushing its
  branch and handing off.**
- `herd-merger` merges the batch **one at a time** onto a single
  integration branch — one at a time so that a later red run is
  attributable to a known merge commit rather than to a blur. It
  resolves the conflicts once, allocates the `D` numbers once in order,
  and writes the board once.
- **The gate is `./scripts/gate.sh`, green on the sha it gated — which
  is not the sha that is pushed, and the difference is load-bearing.**
  The gate runs on the integration branch's tip; the `--no-ff` merge
  commit is created afterwards, because its message *is* the record, so
  it does not exist yet and can never be gated. The condition is
  therefore a receipt for **the merge's second parent** and an
  **identical tree**:

  ```sh
  gated=$(git rev-parse "$local_sha^2")
  receipt-for "$gated"                  || refuse
  git diff --quiet "$gated" "$local_sha" || refuse
  ```

  Measured 2026-10-28: gated `31bb327f…`, pushed `d791dfbc…`, same
  tree, different sha. Written as "the exact sha that is pushed" — which
  is how this file read until that measurement — **no landing is ever
  permitted**, and `scripts/githooks/pre-push` implements this sentence.
  A receipt for any other sha, or a tree that differs, is not a gate run
  for this landing. What `gate.sh` does not cover is below, in *What the
  local merge costs*, and those are known limitations of the method, not
  open questions about it.
- **A green gate does not travel between worktrees**, and *"a green gate
  for the merge's second parent"* reads like something a branch hands
  over. It is not. `gate.sh`'s receipt store is **per worktree** —
  `$HERD_GATE_RECEIPTS`, default `target/herd-gate/`, under a gitignored
  directory that never leaves the agent's own checkout — so a branch
  carries no receipt and a gate run in one worktree is invisible to
  `scripts/githooks/pre-push` running in another. `herd-merger` gates
  **its own** integration tip, which is what its step 7 says; an agent's
  own green gate is evidence about its branch and is never the landing's.
  Nobody copies a receipt in to save a run: the receipt would then name a
  sha and prove nothing about the tree the merger is pushing.
- **The landing record is the merge commit message**, filled from
  `docs/orchestration/templates/landing.md`: the batch, each branch with
  its `ORCH-nnn` and its handoff path, the gate verdict, what was left
  out, what was recorded. `git log --merges main` is the audit trail; it
  needs no network and no GitHub account. **Its permanence is
  contractual, not mechanical, and the difference matters**: a pull
  request body can be edited silently, and a merge commit message can be
  replaced by a force-push. Nothing on GitHub's side stops us doing
  that — measured 2026-10-28, and the measurement is in *What the local
  merge costs* — so what keeps the record is that we forbid rewriting it
  and that `scripts/githooks/pre-push` refuses a push that would. The
  record is better than a pull request body because it is attached to
  the thing it documents and reads with no network, not because a server
  protects it.
- **No docs gate can check the record itself, and this will surprise
  somebody.** `check-docs.sh` and `check-orchestration.sh` can only ever
  see the record's **copy in the handoff**, never the real merge commit
  message: `check-orchestration.sh` builds each of its corpora by
  copying the tracked files **without `.git`**, so no case it runs has a
  commit to read. A green docs gate therefore says the handoff carries a
  filled record and says **nothing** about what `git log -1 --format=%B`
  on `main` actually holds. Only `scripts/githooks/pre-push` is in a
  position to compare the two, because it runs in a real repository at
  push time. Until it does, that comparison is a human's.
- **The handoffs land in the same push as the merge commit**, so the
  record and the per-branch evidence arrive together and a reader who
  was not there needs one `git show`: each branch's handoff under
  `docs/orchestration/history/`, and the merger's own
  `<date>-merge-<batch>.md`.
- **The gate's own output cannot land with them, and saying otherwise
  would be a lie about the order.** The gate must run on the tree that
  is pushed, so nothing can be added to that tree afterwards — not the
  gate's output file, not its verdict, not the post-push run id. The
  merger therefore writes and commits its handoff **before** the gate,
  with its *Gates* section naming the receipt `gate.sh` will write for
  the gated sha; the verbatim output goes to the receipt and to the
  merger's report, and the **next** landing is what carries it into the
  repository if it is wanted there. Same for the post-push run id: the
  handoff and the report get it, and the next landing's board edit
  records it. Two pushes for one batch is the one thing this section
  forbids, and no bookkeeping is worth it.
- The batch is **everything ready when the merger starts**, not a fixed
  count. That is the orchestrator's call, recorded here so that it is not
  re-litigated.
- Each branch's evidence stays **its own**, in the record as in the
  handoffs. A batch is not permission for one blurred summary: a reader
  must still see what each change proved on its own.
- **The merged board is read line by line, not only for conflicts.**
  Required of `herd-merger`: once the batch is merged, take the exact
  line each branch wrote into `board.md` and require it **verbatim** in
  the merged file; then check the other direction — the merged board
  differs from each branch's version only by the other branches' own
  hunks. Batch 3 had **no conflict at all**: three branches wrote more
  than three lines apart, so git merged every hunk without asking. Its
  danger was therefore not a hurried conflict resolution but a board
  merge nobody had to read, and a silent revert with nothing to resolve
  is caught by this check and by nothing else. It takes a minute.
- **The post-push run is recorded, then read.** Immediately after the
  push, `herd-merger` puts its id in its **report** to the orchestrator
  and in its handoff's *Resume point*; the board gets it at the **next**
  landing, for the reason in the bullet above. It does not wait for the
  verdict — that wait is the nine minutes this change exists to stop
  paying — unless it judges a landing risky, and it says which it did.
  The next session start reads
  `gh run list --workflow ci.yml --branch main --limit 5` after the
  board and the last handoff, and a `failure` becomes a new board row
  before any new work is briefed. No automatic GitHub issue: issues are
  Alexa's.
- **Getting here from where we are** is four dated steps, in order, and
  nobody improvises them: *Not an option*, below. Any pull request still
  open when the last of them is done is closed naming the landing that
  replaces it, and its branch stays.

**Unchanged.** Never `--no-verify` — it is what bypasses
`scripts/githooks/pre-push`, the only mechanism standing in front of
`main` — never force-push, never a red landing, never `--admin`. One
integration at a time: a batch **is** one integration, larger. The tag
is the integrator's since 2026-10-29 (**D212**), one act after the push
and never inside it; a release's own notes stay Alexa's (D210). The
orchestrator
still never merges or pushes, and that one is **contractual**:
`orchestrator-guard.sh` denies `git merge` to the main session but was
probed on 2026-10-28 and lets `git -C <dir> merge` and
`git -C <dir> push origin main` through, so it is a guard with a known
hole and not an enforcement. The hole is given to the agent writing the
push guard; until that lands, nothing outside this sentence stops the
orchestrator pushing, and the orchestrator does not push.

**The three refusals of 2026-10-27, with their dates.** A refusal of
Alexa's is history, not a draft: one of these was adopted the next day
and is marked, not deleted.

- **Testing only what was touched. Still refused, 2026-10-27.** The
  consumer's case is seven hours of tests per change; ours is about eight
  minutes. Working out which tests a change affects costs more than it
  saves at that size, and is sometimes wrong.
- **Making this workstation's local run the gate. ADOPTED 2026-10-28**
  (ORCH-120, Alexa's words above), and reaffirmed the same day as an
  obligation rather than an option — *Not an option*, below, carries
  those words, and the test defect that kept this host from a green run
  until it was fixed the same day. The reasoning
  that refused it on
  2026-10-27 stands below as it was written, with its two factual errors
  marked where they occur, because what changed is the facts it rested
  on and not the judgement:

  > Not a matter of principle but a measured fact of 2026-10-27: the
  > suite is red here for host reasons [**no longer true**] — ORCH-069
  > (constants written for a 17 GiB machine) and ORCH-107 (19 of 21
  > failures were timeouts under load, not wrong values) — and skips are
  > per host, 12 here against 15 on CI [**the 15 is wrong**], so judging
  > on the local number is judging a different suite. Three agents each
  > spent time that day proving failures were not theirs. This becomes
  > **reconsiderable once ORCH-069 and ORCH-107 are fixed**: it is a
  > sequence, not a doctrine.

  The sequence it named arrived: ORCH-069 landed (PR #102) and ORCH-107
  landed (batch 4), which is what put the question on the board as
  ORCH-120 rather than leaving it closed. **The suite is no longer red
  here for host reasons** — and, since 2026-10-28, green at rest: a
  full-workspace run was red that morning on three `real_herd` timeouts
  and the test defect behind them was fixed the same day (*Not an
  option*) — and the skip figures were wrong —
  **12 here
  at rest, 14 here under load** (`flood-fifty` and `flood-two-fifty`
  skip themselves above a one-minute load of 8), against **14 on CI**,
  measured (ORCH-116, run 40538722315). The *"15 on CI"* came from run
  40439537929, a **red** run that printed *"AT LEAST 15 assertion(s)"* —
  a floor from an incomplete run, which is exactly what D179 exists to
  label as such.
  **The point that clause was making survives its own numbers and is
  now the design of the gate:** a skip count is not a quantity anyone
  can reason about, and a skip list is. `gate.sh` therefore diffs the
  run's skip lines against a committed expectation and never judges a
  count.
- **A nightly full pass. Still refused, 2026-10-27**, and this project
  still does not have one: **the piece that is missing**, a known gap,
  not an oversight. Since 2026-10-28 it is a **larger** gap. The
  post-push run on `main` is no longer a second opinion behind a
  required check, it is the only thing a second machine ever says about
  a landing, and nothing at all catches GitHub-side drift — an action
  version, the runner image — between two landings. If a scheduled run
  is added to `ci.yml`, this clause is updated in the same landing: a
  refusal of Alexa's is not undone by a workflow file.

## What the local merge costs

**These are known limitations of the method we are adopting, not open
questions about whether to adopt it** — that was settled on 2026-10-28
(*Not an option*). Each one is something the method no longer does for
us and somebody now has to, and each is written here rather than in a
handoff nobody re-reads. Two of them were measured the day the method
was adopted and corrected a claim the plan had made in our favour; they
are kept in the corrected form, because a limitation we can name is
cheaper than one we meet later.

- **The second host leaves the critical path.** A green landing used to
  mean two machines agreed. It will mean one machine agreed and the
  other says so about nine minutes later, to whoever reads it. ORCH-069
  is the worked example of the two disagreeing: an assertion written for
  a 17 GiB machine was red here and green on CI. That class of
  difference will be found **after** the landing.
- **Neither host checks the twelve confinement assertions, and the
  dramatic version of this claim is false (measured 2026-10-28).** All
  **twelve** skips measured here read *"cgroup v2 delegation is
  unavailable: this runner cannot create a cgroup under
  `/sys/fs/cgroup/chatons.slice/chatons-herd-claude-831000.scope`
  (Permission denied (os error 13))"* — an agent on this workstation
  cannot run them either, so it is **not** true that CI cannot check
  invariant 2's confinement and this host can. The real difference
  between 12 here and 14 on CI is two CI-only skips, both
  `the_io_weight_verdict_follows_the_device_the_job_works_on` (*"no
  device offers bfq"*, *"no ZFS mount"*), which do not skip here because
  `/mnt/ssd_980` has blk-iocost enabled. **The honest statement is the
  narrow one: this host runs two assertions CI cannot, and neither host
  runs the twelve.**
- **The gate measures a cgroup slice, not a host (measured
  2026-10-28).** In-test the runner announced *"4.9 GiB rather than 31.1
  GiB, because /proc/self/cgroup is enclosed by memory.high 18 GiB on
  /chatons.slice, which already holds 13 GiB"*. Two agents on this same
  workstation, in different slices or at different moments of the
  consumer's memory use, **do not run the same suite**. A single-host
  gate is really a single-slice-at-one-moment gate, and nothing in this
  contract may imply otherwise.
- **A machine's refusal becomes an agent's judgement.** This is the
  largest of them. `required_status_checks` could not be talked round;
  `scripts/githooks/pre-push` is bypassed by `--no-verify` and a written
  contract can be misread. We have no self-hosted remote and therefore
  no server-side hook available, so this loss is structural, not an
  oversight.
- **The server-side protection we thought we were keeping does not bind
  us (measured 2026-10-28).** The branch protection on `main` is real,
  and `enforce_admins` is **false** while the project's only push
  credential (`gh auth git-credential`) is a repository admin:

  ```
  $ gh api repos/:owner/:repo/branches/main/protection
  "required_status_checks": {"contexts":["check","docs"]},
  "required_pull_request_reviews": {"required_approving_review_count":0},
  "enforce_admins": {"enabled": false},
  "allow_force_pushes": {"enabled": false},
  "allow_deletions": {"enabled": false}
  $ gh api user --jq .login
  tomasalexandre18
  $ gh api repos/:owner/:repo --jq .permissions
  {"admin":true,"maintain":true,"pull":true,"push":true,"triage":true}
  ```

  So `allow_force_pushes: false` and `allow_deletions: false` hold
  against everyone **except** the account that would do the pushing.
  Every such sentence **in the three files this branch owns** — this
  one, `CLAUDE.md` and `.claude/agents/herd-merger.md` — is rewritten
  rather than quietly dropped. **It is not the only place they live:**
  `docs/orchestration/permissions.md`, which this file points at for
  *what is technically enforced*, still says at its line 46 that
  force-push is refused by branch protection, and its lines 5 and 42-46
  describe the pull request lane as the enforcement. That file is
  corrected on branch `method-permissions-truth` and **lands in the same
  batch as this one**; until both are on `main` the project states two
  different things about the same protection. Turning `enforce_admins`
  on would make the old sentences true again and is Alexa's act; until
  then the protection of `main`'s history is the contract plus
  `scripts/githooks/pre-push`, a client-side hook that `--no-verify`
  bypasses. This is the honest shape of the biggest loss above, not a
  separate one.
- **Alexa loses the URL he could open before it landed — by his own
  choice, 2026-10-28.** He was asked whether rule changes (`CLAUDE.md`,
  the invariants, `.claude/`, a release cut) should keep a pull request
  so that he could look before `main` moved, and he answered no.
  Everything lands locally, the rules included, and he reads `main` when
  it suits him. **Nothing waits for his review**, and that is a real
  reduction in oversight, written down as one rather than dressed up:
  the mitigation is only that the orchestrator names the integration
  branch and the landing record in its report, so that `git show` is one
  command away.
- **A post-push run on `main` has never once been read, which is the
  argument for the reading duties and not against the method.** Run
  40439537929 is the proof: a red `check` on `main` on 2026-10-27, after
  the merge of PR #91, and it appears in neither `board.md` nor
  `docs/status.md`. It went unread while the pull request gate was in
  front of it, so removing that gate does not create the problem — it
  removes the cushion that made ignoring it survivable. The mitigation
  is three duties and no mechanism, and they are duties precisely
  because nothing enforces them: the merger records the run id, the next
  session reads the verdict, a failure becomes an `ORCH-nnn` row.
- **Review ergonomics.** No diff view, no inline comment, no
  `gh pr checks --watch`. `herd-reviewer` already writes a file and the
  orchestrator already reads `git diff`, so this one falls on Alexa and
  the orchestrator rather than on the agents.
- **"GitHub is just storage" is not literally true, and acting as if it
  were would be the expensive mistake.** GitHub leaves the path of a
  change and stays: the release factory (`release.yml` — the static
  musl binaries, the hub image archive, the two version numbers an
  operator asks for: D162, D168), the hotfix lane (`release-dev.yml`,
  D172), the force-push and deletion protection on `main` (which binds
  everyone but our own admin credential, above), the push
  credential (`origin` is an `https://` remote whose helper is
  `gh auth git-credential`, so a session with no `gh` login cannot push
  at all), and the only second machine we have.
  `.github/workflows/release*.yml` is not touched by this change and is
  not to be deleted by a later one.
- **One batch must be exactly one push**, and the pull request used to
  make that automatic. It is now a discipline: *What a landing is*.

## When the post-push run on `main` is red

A procedure, because *"we will look into it"* is how run 40439537929
was lost.

1. `gh run rerun --failed` **first**. One command, no reasoning, and it
   separates a flake from a regression. 40439537929 was one timing test
   (`herd-hub/tests/follower_alive.rs:230`) and `main`'s next run was
   green with no change in between.
2. If it reproduces, the landings since the last green run are
   `git log --merges --oneline <last-green-sha>..main` — one merge
   commit per batch, which is why a landing is one `--no-ff` merge and
   one push.
3. Bisect locally over those merge commits, running only the failing
   target and never the whole suite: one worktree, `git bisect`,
   `./scripts/bench-preflight.sh` first.
4. **Fix forward by default.** Revert only when the regression blocks
   other work or a release is pending: `git revert -m 1 <merge-commit>`
   on a branch, gated locally, landed like any other landing — a revert
   is an ordinary landing and gets no shortcut. History stays
   append-only because we keep it so and `pre-push` refuses the rest,
   not because the server refuses it (*What the local merge costs*).
5. A red `main` **blocks a release**, because D162's artefacts are built
   from it, and does **not** block the next landing unless that landing
   touches the same area. Both halves are written down so that neither
   is invented under pressure.

## Permissions and escalation

- Alexa decides: milestone order, invariants, ordering a release, its own
  notes (D210), GitHub issues, anything on the consumer's side. The merge
  is `herd-merger`'s — Alexa's standing authorisation of
  2026-10-24, verbatim *"non il peux merge par defaut"* (ORCH-001),
  re-granted 2026-10-28 on the condition that replaced the old one: a
  green `./scripts/gate.sh` for the **gated** sha — the merge's second
  parent, trees identical (*What a landing is*) — plus a filled
  landing record. The old condition — a CI-green pull request with its
  Evidence filled — is gone with the pull request itself. **The tag is
  the integrator's too since 2026-10-29** (**D212**), on those same two
  conditions and as a separate act after the push. What landing a version
  branch still does not do is write the release's own notes (D210).
- A question for Alexa is one item in the board's *Waiting* section:
  question, recommendation, impact, fallback, date. Work that does not
  depend on it continues. Nobody invents the answer.
- A permission prompt an agent cannot satisfy is reported in its handoff,
  never worked around.

## Execution goes through Herd

`CLAUDE.md`'s *Commands* carries this rule in seven lines, from Alexa's
direction of 2026-10-30: *« a partir de maitenant que du herd, plus
aucune execution local, dois etre ecrit noir sur blanc dans CLAUDE.md et
si il marche pas il te remonte le problemme »*, *« execution local est
une exception pas la norme »*, *« tu peux donc relancer plus d'agent une
fois que tu est sur »*. Here is the recipe, and what the rule does not
buy.

- **Submit with the wrapper**, `/mnt/ssd_980/herd/queue/bin/herdctl`.
  Bare `herdctl` has `ws://127.0.0.1:8787` compiled in
  (`herdctl/src/lib.rs`, `DEFAULT_HUB_URL`) and on this host that is the
  **consumer's production hub**: a call that forgets `--hub` does not
  fail, it submits to their production. The wrapper supplies our hub
  (`ws://127.0.0.1:8791`) and the requester token by environment, so an
  explicit `--hub` or a `HERD_HUB_URL` already set still wins, and an
  empty one counts as absent. If its pinned binary is gone it prints the
  path it looked for and exits **125**, herdctl's own "no verdict" code.
  It never falls back to a `herdctl` on `PATH`, because that fallback is
  exactly how a call reaches 8787.
- **`--label build` and `--memory-mib` on every heavy submission.** The
  label is how the hub finds a runner allowed to build; the memory
  figure is what it admits the job against. Declare nothing and the job
  is admitted at **0 MiB**, so the only bound left is the slot count: a
  twelve-core `gate.sh` job ran 376 s and wrote ~13 GB having declared
  no memory at all (measured by the agent that proved the path,
  2026-10-30, and reported to the orchestrator; no record of it is in
  this tree). The hub is honest about what it was told, and the
  requester told it nothing.
- **What the slot count is, measured on our instance.** One runner,
  `queue-local`, declaring `build` as `counted` with **2 slots**
  (`/mnt/ssd_980/herd/queue/etc/capabilities.json`, read 2026-10-30),
  under `--job-memory-max 6GiB` and `--host-memory 16GiB` with a
  `--host-memory-floor 4GiB`. So the queue admits **two** build jobs at
  once — on a twelve-core host, up to **twenty-four `rustc`** if neither
  declared its memory. **The queue bounds jobs, not cores**, and the
  figure is 2: an earlier reading of a hub ceiling of 24 was two slots
  times twelve cores, and 24 is a count of compilers, not of jobs.
- **`./scripts/bench-preflight.sh` still runs first.** Herd **records**
  load and does not gate on it: every placement sentence says *"recorded
  and not a gate (E-32)"*, and `herd-hub/src/matcher.rs` states there is
  no `if load` anywhere in the module, by design and by test. The
  preflight's own ceiling is `ncpu * 0.75` — **9** on this 12-core host
  — and nothing in the hub reads anything equivalent to the **room left
  in the systemd slice**, which is what the preflight's check 4 reads
  and what decided five lost suite runs on 2026-10-29. Drop this clause
  and the rule reads as a guarantee the instance does not give.
- **Take `--verdict-to <FILE>` on every submission**, because
  **`herdctl` has no command that reads a finished job's record**:
  `run`, `submit`, `wait`, `cancel` and `runner` are the whole surface.
  Without it the figures are reachable only through the hub's **state
  root** — fine on our own instance, and **D161 forbids it on a
  consumer's**.
- **Two frictions to plan for.** A job's whole log arrives only at
  completion, so a step that goes red is invisible until the end: do not
  read silence as progress. And a job gets **no workspace** — it `cd`s
  into the requester's worktree, so `gate.sh`'s receipt lands in
  `<worktree>/target/herd-gate/` even when `CARGO_TARGET_DIR` points
  elsewhere, which is why `pre-push` can find it.
- **A job that fails is reported, never re-run locally.** A local run is
  permissible only as a **named control**: the agent says it is a
  control, and why. That is what *« execution local est une exception
  pas la norme »* means in a handoff.
- **What the rule does not buy.** Our runner has **no cgroup**, so Herd
  today **orders** work and does not **bound** it. `--job-memory-max`
  falls back to `RLIMIT_AS` (D23), a per-process address-space limit and
  not a job-wide ceiling, and a record's resource figures are what the
  runner could *see*: a **382-second** `gate.sh` job recorded
  `peak_memory_bytes` of **3,141,632** — 3.1 MB — and `bytes_written` of
  287 MB for a run that wrote about 11 GB (ORCH-268). Alexa has directed
  that job limits are not an obligation and that our load does not need
  them, so this is **not** a blocker for the rule; it is a thing a
  reader must not misread as protection. Whether a record should say
  *unmeasured* is **ORCH-268**, open and his.

## Concurrency and host rules

- **Since 2026-10-30 the queue answers "how many at once", not a hand
  count** (*Execution goes through Herd*, above): heavy work is
  submitted, the hub admits it when a build slot is free, and the
  per-job judgement is `bench-preflight.sh`'s. The bullet below still
  holds for anything that runs outside a job, and its figures are the
  ones `CLAUDE.md` now points here for.
- How many heavy agents (compiling or running the suite) run at once is a
  judgement on the machine, not a count: look at the load average, the
  free memory **and the free disk** before launching another, at one
  **`debug` build tree carrying test binaries** each — **~12–14 GB**.
  Seven were measured on 2026-10-30, every one built by
  `./scripts/test.sh` or `gate.sh`; two of the seven are in the tree,
  `history/2026-10-30-pack-fetch.md:566` at 13 GB and
  `history/2026-10-30-pack-registry.md:629` at 12 GB, and the other five
  are in agent reports of the same day and are unrecoverable (which is
  why the handoff now records the figure: *Handoff and resumption*).
  **The profile is named before the number because the other profile is
  forty times smaller**: a `--release` build of the whole workspace at
  `v0.4.1` is **283 MiB** — 292,415,975 bytes, 101 crates, 52 s, one
  measurement, 2026-10-30. A heavy agent builds `debug` with tests, so
  the `debug` figure is the one that bounds how many fit, and correcting
  it *down* to 283 MiB would halve the ceiling on a number that is true
  and answers a different question. The figure this bullet gave until
  2026-10-30, ~8.5 GB per tree, was low by a factor of 1.4 to 1.6 and
  named no profile at all (ORCH-250). While the first consumer is
  stopped — it is the machine's main source of load — more than two are
  allowed (Alexa, 2026-10-27, with five running on this 12-core, 31 GiB
  host at load 1.21). The ceiling of **two** returns the day the
  consumer resumes. Any number of read-only or prose agents, always.
- **An agent removes its own `target/` as soon as its branch is on the
  origin.** Not housekeeping: `/mnt/ssd_980` is the same filesystem the
  first consumer's production writes to — the hub's docker volume
  (`herd_herd-state`, 107 GB) and the runner's data root
  (`/mnt/ssd_980/herd-runner`, 69 GB) are on it, and the hub's own output
  budget is 200 GB against the ~344 GB free. On 2026-10-27 six finished
  tasks had left 48 GB of abandoned build trees behind, a seventh of that
  free space; it had happened every session. An agent that keeps its
  build tree is competing with the consumer's hub for disk. The branch is
  the deliverable and the build tree is not, so it is the agent's own job
  at handoff, not the orchestrator's afterwards. The condition is
  **pushed**, not merged — an agent never waits for `herd-merger` — and
  never before its own gates have run. It removes the `target/` in its
  **own** scratchpad subdirectory and nothing else: not another agent's,
  not the orchestrator's checkout, and by a literal path, never one
  assembled from a shell variable that could be empty.
- One `herd-merger` at a time; the board shows which batch it holds. A
  batch is one integration, larger, never several integrations at once.
- The orchestrator's checkout `/mnt/ssd_980/herd-project` is **stale by
  construction** — nobody commits there and the guard denies the main
  session the `git merge` that would advance it, though only in that
  spelling: it is **not** blocked from fetching, and `git -C <dir>`
  walks past the guard entirely (both measured 2026-10-28, and both are
  corrections to what this bullet used to say; the hole is in
  *Unchanged*, above) — so it is never a source for the current
  state of `main`: read `origin/main`, in your own worktree or with
  `git show origin/main:<path>`. Why, and what it cost on 2026-10-27:
  `templates/brief.md`. It cost again on 2026-10-28: a brief quoted a
  sentence of `CLAUDE.md` that had not existed since 2026-10-27, read
  from that checkout, which was **217 commits behind `origin/main`** (26
  by first parent) and clean. ORCH-031 closed the half where an agent
  *writes* there; this is the half where the orchestrator *reads* there.
- **Creating a worktree from that checkout is not a write in the sense
  of the rule above.** `git worktree add` registers metadata under its
  `.git/worktrees/` (85 entries on 2026-10-28) and `git fetch` writes
  `FETCH_HEAD`; every agent's brief asks for both. What is forbidden is
  changing what that checkout *says*: a commit, an edit, a staged file,
  a moved branch.
- **`core.hooksPath` is the second named exception, it is one setting
  for the whole repository — not per worktree — and it is already set.**
  Measured 2026-10-28 on git 2.54.0:
  `git -C <worktree> config core.hooksPath …` writes the **shared**
  config of `/mnt/ssd_980/herd-project`, arming every worktree at once
  (85 of them that day), and `--worktree` is refused outright because
  `extensions.worktreeConfig` is unset. So it is **one deliberate act,
  not something an agent sets for itself in passing**, and it was done
  on 2026-10-28 by the agent that corrected `permissions.md`, after it
  had established that `/mnt/ssd_980/herd-project/.git/hooks/` holds
  nothing but `*.sample`. That ordering is deliberate and its cost is
  nil: the setting is **inert until the batch lands**, because it points
  at a file that arrives with the batch, and it displaces no hook that
  was doing anything.
- **The path is absolute, and that is not a style choice.** A relative
  `core.hooksPath` resolves against each worktree's own root, so a
  worktree created from a pre-landing sha would resolve it to a
  directory that does not exist — and pointing `core.hooksPath` at a
  missing directory **disables `.git/hooks` and supplies nothing in its
  place**, which is strictly worse than leaving it unset. One absolute
  path resolves to the same file from every worktree:

  ```sh
  test -x /mnt/ssd_980/herd-project/scripts/githooks/pre-push &&
    git -C /mnt/ssd_980/herd-project config core.hooksPath \
      /mnt/ssd_980/herd-project/scripts/githooks
  ```

  The guard and the value then name the same place, which the relative
  form did not: it tested the hook in the setting agent's own worktree
  and armed every other one.
- **`herd-merger` advances that checkout as the last thing it does**,
  once `main` has moved. Its safety is in the preconditions, and they
  are **commands, not prose**, because `--ff-only` does not supply them.
  **The commands live in one place only**, step 10 of
  `.claude/agents/herd-merger.md`, and are deliberately not copied here:
  this procedure had two copies on 2026-10-28, one of them was fixed and
  the other kept printing nothing, which is exactly how a silent-skip
  defect comes back. What follows is why those checks exist; what to run
  is in the role file.

  Both checks were measured on 2026-10-28 and **both failure modes are
  real**: a dirty file the incoming commits do not touch is carried
  straight through by a fast-forward (`Fast-forward`, exit 0, still
  dirty), and with any branch other than `main` checked out,
  `git -C "$d" merge --ff-only origin/main` silently fast-forwards
  **that** branch to `origin/main` while local `main` never moves — so
  another party's branch pointer is rewritten and a handoff reporting
  "moved from X to Y" names the wrong ref. `fetch origin main:main` is
  not a substitute: it refuses while `main` is checked out and moves
  `main` under a different HEAD when it is not. When either check stops
  the step, the merger reports it in the handoff and **leaves the
  checkout alone** — never `reset --hard`, never `checkout -f`, never
  `stash`. It is not the merger's to repair, only to advance, and a
  failure there never fails the landing.
- **Why the merger and not a hook**, since this is exactly the kind of
  thing a later agent will want to improve: a `SubagentStop` hook
  matched on `herd-merger` would work — per-agent hooks are supported
  and this project already uses one, the `PreToolUse` guard in
  `.claude/agents/herd-release.md` — but a hook doing it means the
  **session's own machinery** writes the repository, which is what
  `.claude/hooks/orchestrator-guard.sh` exists to prevent and what
  `CLAUDE.md`'s *Roles* forbids. An agent writing the repository is the
  normal case and needs no new mechanism and no exception. The privilege
  stays where the contract already puts it, with its conditions beside
  it in `.claude/agents/herd-merger.md`.
- Never two agents on the same worktree, scratchpad subdirectory or
  decision entry. When two tasks would touch the same file, the second is
  based on the first's branch.
- `pkill -f`, `killall`, `kill` by name are forbidden; an agent kills
  only PIDs it started and lists them in its handoff.
- **Ownership, not age, decides what may be signalled, and ownership is
  read from a process's full `args`.** The rule above is correct and on
  2026-10-30 it was kept to the letter and was **not sufficient**. Three
  things this host measures, each of which makes the rule a fact rather
  than advice:
  1. **uid 1002 (`server`) is shared with the first consumer's
     production.** Their nightly's requesters run under it; their hub
     runs as uid **1000** and their runner as uid **986**. So a check
     that excludes only those two known PIDs **proves nothing about the
     ownership of a third**, and `comm` is `herdctl` for our own test
     fixtures and for their production alike. Our uid is not our mark,
     and neither is the process name.
  2. **Read a process's full `args`, and say whose it is, before
     signalling it.** Age, PID and name are not provenance. On
     2026-10-30 the orchestrator killed **78** processes under our own
     account, believing an agent's report that they were a Herd agent's
     leak; they were the consumer's production nightly requesters,
     `herdctl wait --outputs-to
     /mnt/ssd_980/chatons/agents/nightly/herd-j<id>`. The filter used
     was **age** plus those two known PIDs: careful on the wrong axis,
     twice. One `ps -o args` would have shown the `chatons/` path, and
     it was run an hour late. **The error is the orchestrator's**, not
     an agent's, and it is not softened here because the next reader
     needs to know which seat it was made from.
  3. **A report that declares its own lack of provenance is not
     provenance.** The agent that flagged those processes wrote *"worth
     someone with provenance cleaning up; I had none"* — and that
     sentence was passed on as fact. An agent naming what it does not
     know is doing its job; treating the report as the reading is the
     failure, and it belongs to whoever acted on it.
- **The detector, because a rule with none is advice: `HERD_JOB_MARKER`
  in `/proc/<pid>/environ`.** The runner builds a job's environment from
  scratch — only `PATH`, `LANG`, `LC_ALL`, `TZ` and `TERM` are carried
  through — and sets this one variable of its own
  (`herd-runner/src/reclaim.rs`, `MARKER_VARIABLE`;
  `docs/runner-confinement.md`), so **only a job's own processes carry
  it**, and they keep it across `fork`, `exec`, a double fork, `nohup`
  and `setsid`. Its value says which: `herd-job-…` for a job,
  `herd-probe-…` for a probe reading. The sweep is six lines and is
  written here rather than left in somebody's scratchpad, which is
  where the first copy of it was:

  ```sh
  for p in /proc/[0-9]*; do
    m=$({ tr '\0' '\n' < "$p/environ"; } 2>/dev/null |
          grep '^HERD_JOB_MARKER=')
    [ -n "$m" ] || continue
    printf '  pid=%s %s args=%s\n' "${p#/proc/}" "$m" \
      "$({ tr '\0' ' ' < "$p/cmdline"; } 2>/dev/null)"
  done
  ```

  **Prove it fires before trusting its silence.** The first copy was
  proved on three live processes before its zero was believed (2026-10-30,
  by the agent that deployed our own instance), and its author was one
  step from reporting a false defect *and* from killing the consumer's
  `ram-janitor.sh`, which was doing its own `sleep 120` under our shared
  uid — the second near-miss of the same shape in one morning, and the
  argument for writing any of this down. **Its one known limit, stated
  rather than discovered:** a process that erases its own environment is
  not found (`docs/architecture.md`, the kill's weaker case). So a
  silence from this sweep is evidence about Herd's own jobs and never a
  licence to signal what it did not name.
- No `cargo` build or test for a prose change (gotcha `docs-only-gates`).
- The consumer's production runs on this host: no job submission, no
  container, no service, no `/etc/nixos`, ever (D161).

## Handoff and resumption

A handoff is written by the agent that did the work, at the end of every
task, successful or not, into `docs/orchestration/history/`. It carries:
branch, commit, what was done, what was not and why, gate output or the
sentence "not run", skips, risks, decisions needed, exact resume point.
A fresh session resumes from the handoff and the board, never from a
conversation summary. The orchestrator's report to Alexa repeats the
handoff's *not done* and *decisions needed* verbatim.

**And one measurement, from every step that built anything.** Before it
removes its build tree a step runs `du -sh "$CARGO_TARGET_DIR"` and
records the figure **with its build profile** — a `debug` tree with test
binaries, or `--release` — in its *Processes* section, beside the path it
deleted. One line per handoff. The reason is ORCH-250: of the seven trees
measured on 2026-10-30, only two are in the tree
(`history/2026-10-30-pack-fetch.md:566` and
`history/2026-10-30-pack-registry.md:629`) and the other five live in
agent reports alone and cannot be recovered, so the figure that bounds how
many heavy agents fit had become hearsay by the next morning. A figure
with no profile on it is worse than no figure: `debug` with tests and
`--release` differ by a factor of forty on this workspace, so a bare
number invites a later "correction" to the other profile's true value.

## Failure

- A red gate blocks; nobody edits history or a test to turn it green.
- **A gate's output goes to a file, and the file is what you read** —
  `./scripts/check-docs.sh > out.txt 2>&1`, never a pipe to `head`,
  `tail` or anything that truncates. A gate's output is the evidence,
  and an intermittent failure's first sample is the one you cannot get
  back: on 2026-10-27 the first two samples of ORCH-111 went into
  `tail -2`, and it took about thirty further runs to recover what they
  would have shown.
- A skip is printed as a skip (D27) and copied into the handoff as such.
- A disagreement between an agent and its brief is a finding in the
  handoff: the agent stops, states the evidence, and the orchestrator
  arbitrates or asks Alexa. An agent is never asked to rewrite a finding.
- An agent that crossed its write paths reports it; `herd-merger` drops
  the out-of-scope hunks before integration.
