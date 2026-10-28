# Orchestration workflow

How a request becomes merged work, who may do what, and what every agent
owes before it stops. `CLAUDE.md` is the contract; this is the procedure.

## Roles and rights

| Role | Does | Never |
|---|---|---|
| orchestrator (main session) | receives the goal, reads the board, picks agents, writes briefs, reads evidence, asks Alexa one question at a time, reports in French | edit a repository file, commit, merge, resolve a conflict, tag, release, declare a skip a pass, report an agent's claim unverified |
| `herd-planner` | analyses a request, lists alternatives and hypotheses to refute, cuts bounded tasks | modify the product, merge, settle an owner decision |
| `herd-step` | one bounded task on its own branch and worktree, with the gates the task needs, evidence and a handoff; pushes the branch | leave its write paths, push to `main`, touch another agent's worktree, or the running instance |
| `herd-merger` | integrates a batch of branches, one at a time onto one integration branch: conflicts, the local gate, D-number allocation, board and status update, then **one** `--no-ff` merge commit on `main` carrying the landing record, pushed under standing authorisation ORCH-001 | bypass a gate, push `main` without a green gate for that exact sha, decide a specification or invariant change, operate Herd |
| `herd-release` | prepares a release: checklist, version, body, evidence; simulation only until Alexa orders a tag | tag, push a tag, publish, deploy, land its own branch |
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
   before the push — and the output quoted in the handoff is that run's:
   a handoff is itself a docs file, so a gate run before it was written
   tested a different tree, and under one landing per batch one ungated
   handoff reddens the batch's own gate run, where it blocks every
   other branch in it. **Then** the branch is pushed. **No pull request
   is opened, by anyone** (*One landing per batch*): the branch and the
   handoff are the delivery. A worktree gets `git config core.hooksPath
   scripts/githooks` when it is created, so that the repository's own
   `scripts/githooks/pre-push` applies in it.
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
   `docs/status.md` written once. It then runs `./scripts/gate.sh` on
   that branch, merges it `--no-ff` onto `main` in its own worktree with
   the landing record as the merge commit message, and pushes `main`
   **once**: Alexa's standing authorisation (ORCH-001, 2026-10-24,
   re-granted 2026-10-28) covers any landing whose gate is green on the
   exact sha pushed and whose landing record is filled. `main` moves no
   other way, and no pull request is opened. A tag is never part of this
   step. The rule, what a landing is, what it costs and the three method
   changes refused with it: *One landing per batch*, below.
8. **Release.** On Alexa's order, `herd-release` prepares the checklist
   and pushes the `cut-v<version>` branch that bumps the version;
   `herd-merger` lands it, as for any other branch. The tag itself is
   pushed by whoever Alexa names in that order. Nothing in this project
   deploys.

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
`herd-merger`, not for a rule change, not for a release cut. There is no
remote collaborator: Alexa and this project's agents are the whole set
of authors, so a pull request bought review from nobody and cost about
nine minutes of CI per batch on the critical path (PR run 40562916742,
8 m 54 s, 2026-10-27).

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
- **The gate is `./scripts/gate.sh`, green on the exact sha that is
  pushed.** It runs the steps of the `check` and `docs` CI jobs, in the
  same order, on this workstation, and leaves a receipt named for the
  sha; a receipt for another sha is not a gate run for this one. What it
  does not cover is below, in *What the local merge costs*.
- **The landing record is the merge commit message**, filled from
  `docs/orchestration/templates/landing.md`: the batch, each branch with
  its `ORCH-nnn` and its handoff path, the gate verdict, what was left
  out, what was recorded. `git log --merges main` is the audit trail; it
  needs no network and no GitHub account, and it cannot be rewritten
  after the fact, because force-push to `main` stays refused
  server-side. A pull request body could be edited silently, a merge
  commit message cannot — this is the one part of the change that makes
  the record stronger rather than weaker.
- **The verbatim output stays in files in the repository**, as before:
  each branch's handoff under `docs/orchestration/history/`, and the
  merger's own `<date>-merge-<batch>.md` carrying the full `gate.sh`
  output file and the skip list. They land in the same push as the merge
  commit, so the record and its evidence arrive together and a reader
  who was not there needs one `git show`.
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
- **The post-push run is recorded, then read.** `herd-merger` puts its
  id in its handoff and in the board row **immediately after the push**
  and does not wait for the verdict — that wait is the nine minutes this
  change exists to stop paying. It may choose to wait when it judges a
  landing risky, and it says which it did. The next session start reads
  `gh run list --workflow ci.yml --branch main --limit 5` after the
  board and the last handoff, and a `failure` becomes an `ORCH-nnn` row
  before any new work is briefed. No automatic GitHub issue: issues are
  Alexa's.
- **Transition, and it is not ours to skip.** GitHub still requires a
  pull request on `main` today (`required_pull_request_reviews`, plus
  the required contexts `check` and `docs`), so until that branch
  protection is narrowed — Alexa's act, and the first step of this
  change — a push to `main` is refused by the server, not by us. The
  change itself therefore lands through one last pull request. Any pull
  request still open when it does is closed naming the landing that
  replaces it, and its branch stays.

**Unchanged.** Never `--no-verify` — it is what bypasses
`scripts/githooks/pre-push`, which is now the only mechanism standing in
front of `main` — never force-push, never a red landing, never
`--admin`. One integration at a time: a batch **is** one integration,
larger. A tag and a release stay Alexa's word, one act at a time. The
orchestrator still never merges or pushes, and that one is still
enforced from outside the text, by `orchestrator-guard.sh`.

**The three refusals of 2026-10-27, with their dates.** A refusal of
Alexa's is history, not a draft: one of these was adopted the next day
and is marked, not deleted.

- **Testing only what was touched. Still refused, 2026-10-27.** The
  consumer's case is seven hours of tests per change; ours is about eight
  minutes. Working out which tests a change affects costs more than it
  saves at that size, and is sometimes wrong.
- **Making this workstation's local run the gate. ADOPTED 2026-10-28**
  (ORCH-120, Alexa's words above). The reasoning that refused it on
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
  here for host reasons**, and the skip figures were wrong — **12 here
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
  version, the runner image — between two landings.

## What the local merge costs

Here and not only in a handoff, because every one of these is something
the method no longer does for us and somebody now has to.

- **The second host leaves the critical path.** A green landing used to
  mean two machines agreed. It now means one machine agreed and the
  other will say so about nine minutes later, to whoever reads it.
  ORCH-069 is the worked example of the two disagreeing: an assertion
  written for a 17 GiB machine was red here and green on CI. That class
  of difference is now found **after** the landing.
- **A machine's refusal becomes an agent's judgement.** This is the
  largest of them. `required_status_checks` could not be talked round;
  `scripts/githooks/pre-push` is bypassed by `--no-verify` and a written
  contract can be misread. We have no self-hosted remote and therefore
  no server-side hook available, so this loss is structural, not an
  oversight. What survives server-side is the force-push and deletion
  protection on `main`, kept deliberately.
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
- **A post-push run on `main` has never once been read.** Run
  40439537929 is the proof: a red `check` on `main` on 2026-10-27, after
  the merge of PR #91, and it appears in neither `board.md` nor
  `docs/status.md`. We are promoting an unread signal to a safety net.
  The mitigation is three duties and no mechanism — the merger records
  the run id, the next session reads the verdict, a failure becomes an
  `ORCH-nnn` row.
- **Review ergonomics.** No diff view, no inline comment, no
  `gh pr checks --watch`. `herd-reviewer` already writes a file and the
  orchestrator already reads `git diff`, so this one falls on Alexa and
  the orchestrator rather than on the agents.
- **"GitHub is just storage" is not literally true, and acting as if it
  were would be the expensive mistake.** GitHub leaves the path of a
  change and stays: the release factory (`release.yml` — the static
  musl binaries, the hub image archive, the two version numbers an
  operator asks for: D162, D168), the hotfix lane (`release-dev.yml`,
  D172), the force-push and deletion protection on `main`, the push
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
   on a branch, gated locally, landed like any other landing. A revert
   needs no pull request and never did, and history stays append-only —
   which is why force-push protection is the one server-side rule we
   keep.
5. A red `main` **blocks a release**, because D162's artefacts are built
   from it, and does **not** block the next landing unless that landing
   touches the same area. Both halves are written down so that neither
   is invented under pressure.

## Permissions and escalation

- Alexa decides: milestone order, invariants, pushing a tag, publishing a
  release (each act singly), GitHub issues, anything on the consumer's
  side. Pushing a landing whose local gate is green on the exact sha
  and whose landing record is filled is `herd-merger`'s (ORCH-001,
  re-granted 2026-10-28); landing a version branch does not publish its
  release.
- A question for Alexa is one item in the board's *Waiting* section:
  question, recommendation, impact, fallback, date. Work that does not
  depend on it continues. Nobody invents the answer.
- A permission prompt an agent cannot satisfy is reported in its handoff,
  never worked around.

## Concurrency and host rules

- How many heavy agents (compiling or running the suite) run at once is a
  judgement on the machine, not a count: look at the load average, the
  free memory **and the free disk** before launching another, at roughly
  8.5 GB of build tree each. While the first consumer is stopped — it is
  the machine's main source of load — more than two are allowed (Alexa,
  2026-10-27, with five running on this 12-core, 31 GiB host at load
  1.21). The ceiling of **two** returns the day the consumer resumes.
  Any number of read-only or prose agents, always.
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
  construction** — the guard blocks it from fetching — so it is never a
  source for the current state of `main`: read `origin/main`, in your own
  worktree or with `git show origin/main:<path>`. Why, and what it cost
  on 2026-10-27: `templates/brief.md`.
- Never two agents on the same worktree, scratchpad subdirectory or
  decision entry. When two tasks would touch the same file, the second is
  based on the first's branch.
- `pkill -f`, `killall`, `kill` by name are forbidden; an agent kills
  only PIDs it started and lists them in its handoff.
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
