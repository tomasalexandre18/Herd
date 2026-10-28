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

**Refused, for now.**

- **Testing only what was touched.** The consumer's case is seven hours
  of tests per change; ours is about eight minutes on CI. Working out
  which tests a change affects costs more than it saves at that size, and
  is sometimes wrong.
- **Making this workstation's local run the gate.** Not a matter of
  principle but a measured fact of 2026-10-27: the suite is red here for
  host reasons — ORCH-069 (constants written for a 17 GiB machine) and
  ORCH-107 (19 of 21 failures were timeouts under load, not wrong
  values) — and skips are per host, 12 here against 15 on CI, so judging
  on the local number is judging a different suite. Three agents each
  spent time that day proving failures were not theirs. This becomes
  **reconsiderable once ORCH-069 and ORCH-107 are fixed**: it is a
  sequence, not a doctrine.
- **A nightly full pass**, which this project does not have. Not adopted,
  **and the piece that is missing**: a known gap, not an oversight.

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
