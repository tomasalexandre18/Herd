# Orchestration workflow

How a request becomes merged work, who may do what, and what every agent
owes before it stops. `CLAUDE.md` is the contract; this is the procedure.

## Roles and rights

| Role | Does | Never |
|---|---|---|
| orchestrator (main session) | receives the goal, reads the board, picks agents, writes briefs, reads evidence, asks Alexa one question at a time, reports in French | edit a repository file, commit, merge, resolve a conflict, tag, release, declare a skip a pass, report an agent's claim unverified |
| `herd-planner` | analyses a request, lists alternatives and hypotheses to refute, cuts bounded tasks | modify the product, merge, settle an owner decision |
| `herd-step` | one bounded task on its own branch and worktree, with the gates the task needs, evidence and a handoff; pushes the branch | open a pull request, leave its write paths, touch `main`, another agent's worktree, or the running instance |
| `herd-merger` | integrates a batch of branches, one at a time onto one integration branch: conflicts, gates, D-number allocation, board and status update, **one** PR for the batch, and the merge itself (standing authorisation ORCH-001) | bypass a gate, decide a specification or invariant change, operate Herd |
| `herd-release` | prepares a release: checklist, version, body, evidence; simulation only until Alexa orders a tag | tag, push a tag, publish, deploy, open its own PR |
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
   scratchpad subdirectory on branch `<task>`, pushes the branch, and
   ends with a handoff from `templates/handoff.md` saved as
   `docs/orchestration/history/<date>-<task>.md` on that branch. It
   **opens no pull request**: the branch and the handoff are the delivery.
5. **Verify.** The orchestrator reads the diff, the gate output and the
   handoff. It reruns a read-only check when a claim matters. It never
   fixes what it finds; it sends it back or opens a new task.
6. **Review when it is a rule.** A change to `CLAUDE.md`, agents,
   permissions, hooks, `check-docs.sh` or an invariant gets a
   `herd-reviewer` pass before integration; a micro-edit does not.
7. **Integrate.** `herd-merger` takes a **batch** of finished branches —
   everything ready when it starts — and merges them **one at a time**
   onto one integration branch off `origin/main`: conflicts resolved in
   its worktree, `D` numbers allocated once in order (`D-NEXT` → next
   free number, gaps stay), the gates the diff needs, `board.md` and
   `docs/status.md` written once. It then opens **one** PR for the batch
   from `.github/pull_request_template.md`, with **each branch's own
   evidence separately** in the Evidence block, waits for CI, and merges:
   Alexa's standing authorisation (ORCH-001, 2026-10-24) covers every PR
   whose CI is green and whose Evidence block is filled. `main` moves no
   other way. A tag is never part of this step. The rule, and the three
   method changes refused with it: *One PR per batch of branches*, below.
8. **Release.** On Alexa's order, `herd-release` prepares the checklist
   and pushes the `cut-v<version>` branch that bumps the version;
   `herd-merger` opens its PR, as for any other branch. The tag itself is
   pushed by whoever Alexa names in that order. Nothing in this project
   deploys.

## One PR per batch of branches

Alexa, 2026-10-27, verbatim: *"pour le moment on applique uniquement le
une PR = plusieur lot, le reste non"* — answering a proposal modelled on
how the first consumer works. One part of that proposal is adopted; three
are refused. The refusals are written down here with their reason so that
nobody re-adopts them by drift.

**Adopted: one pull request per batch of several steps.**

- One branch per task is unchanged. An agent finishes by **pushing its
  branch and handing off; it does not open a pull request.**
- `herd-merger` takes a **batch** of finished branches and merges them
  **one at a time** onto a single integration branch — one at a time so
  that a later red CI is attributable to a known merge commit rather than
  to a blur. It resolves the conflicts once, allocates the `D` numbers
  once in order, writes the board once, and opens **one** pull request
  for the batch.
- The batch is **everything ready when the merger starts**, not a fixed
  count. That is the orchestrator's call, recorded here so that it is not
  re-litigated.
- The batch PR's Evidence block carries **each branch's own evidence
  separately**. A batch is not permission for one blurred summary: a
  reader must still see what each change proved on its own.
- Superseded single PRs, if any were already open, are closed naming the
  batch PR; their branches stay.

**Unchanged.** CI is the gate: jobs `check` and `docs` green on the
pushed head, Evidence filled, then the merge under ORCH-001. Never
`--no-verify`, never force-push, never a red merge, never `--admin`. One
integration at a time: a batch **is** one integration, larger.

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
  side. Merging a CI-green PR with filled Evidence is `herd-merger`'s
  (ORCH-001); merging a version PR does not publish its release.
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
