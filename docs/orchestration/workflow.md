# Orchestration workflow

How a request becomes merged work, who may do what, and what every agent
owes before it stops. `CLAUDE.md` is the contract; this is the procedure.

## Roles and rights

| Role | Does | Never |
|---|---|---|
| orchestrator (main session) | receives the goal, reads the board, picks agents, writes briefs, reads evidence, asks Alexa one question at a time, reports in French | edit a repository file, commit, merge, resolve a conflict, tag, release, declare a skip a pass, report an agent's claim unverified |
| `herd-planner` | analyses a request, lists alternatives and hypotheses to refute, cuts bounded tasks | modify the product, merge, settle an owner decision |
| `herd-step` | one bounded task on its own branch and worktree, with the gates the task needs, evidence and a handoff | leave its write paths, touch `main`, another agent's worktree, or the running instance |
| `herd-merger` | integrates one branch: rebase or merge, conflicts, gates, D-number allocation, board and status update, PR, and the merge itself (standing authorisation ORCH-001) | bypass a gate, decide a specification or invariant change, operate Herd |
| `herd-release` | prepares a release: checklist, version, body, evidence; simulation only until Alexa orders a tag | tag, push a tag, publish, deploy |
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
   `docs/orchestration/history/<date>-<task>.md` on that branch.
5. **Verify.** The orchestrator reads the diff, the gate output and the
   handoff. It reruns a read-only check when a claim matters. It never
   fixes what it finds; it sends it back or opens a new task.
6. **Review when it is a rule.** A change to `CLAUDE.md`, agents,
   permissions, hooks, `check-docs.sh` or an invariant gets a
   `herd-reviewer` pass before integration; a micro-edit does not.
7. **Integrate.** `herd-merger` takes one branch at a time: rebases on
   `origin/main`, resolves conflicts in its worktree, allocates `D`
   numbers (`D-NEXT` → next free number, gaps stay), runs the gates the
   change needs, updates `board.md` and `docs/status.md`, opens or
   updates the PR with Evidence, waits for CI, and merges: Alexa's standing
   authorisation (ORCH-001, 2026-10-24) covers every PR whose CI is green
   and whose Evidence block is filled. A tag is never part of this step.
8. **Release.** On Alexa's order, `herd-release` prepares the checklist
   and the PR that bumps the version; the tag itself is pushed by whoever
   Alexa names in that order. Nothing in this project deploys.

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

- At most **two** heavy agents (compiling or running the suite) at once;
  any number of read-only or prose agents.
- One `herd-merger` at a time; the board shows which branch it holds.
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
- A skip is printed as a skip (D27) and copied into the handoff as such.
- A disagreement between an agent and its brief is a finding in the
  handoff: the agent stops, states the evidence, and the orchestrator
  arbitrates or asks Alexa. An agent is never asked to rewrite a finding.
- An agent that crossed its write paths reports it; `herd-merger` drops
  the out-of-scope hunks before integration.
