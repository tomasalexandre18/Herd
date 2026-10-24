# Orchestration remediation, step H7 (validation) — handoff 2026-10-24

- **Item:** ORCH-004
- **Agent:** the remediation agent (acting on the H7 mandate)
- **Branch / commit:** `orchestration-remediation` / this commit, based on
  `origin/main` d57f23a
- **PR:** none yet

## Done

Four live `claude -p` sessions were run against a scratch worktree of this
branch, with the guard's trace file on, and the guard was corrected from
what they found. The details are in
`docs/orchestration/permissions.md` "Live verification"; in short:

- The guard's discriminator was confirmed on real payloads: `--agent
  herd-step` logs `agent=herd-step`, an `Agent`-tool agent logs an opaque
  `agent_id`, the main session logs neither.
- V1 (a fresh session asked ten questions about its own contract) answered
  all ten from the files, with citations, including that it must delegate a
  one-line fix and that the chatOnS persona does not apply here.
- V11 (the orchestrator pressed to edit and commit "quickly" itself)
  refused, delegated, and reported the gate failure verbatim instead of
  raising the limit. It also exposed a real gap, now ORCH-031 and
  `gotchas/brief-names-the-worktree.md`: its brief named no worktree, so
  the agent worked in the orchestrator's checkout.
- V7 (a reviewer given three deliberately false premises) refuted all
  three with the line that disproves each, and found two real holes in the
  guard.

Both holes are closed and covered by tests: a `..` segment no longer lets a
path pose as an agent worktree or hide the repository prefix (paths are
normalised before matching), and `cd <checkout> && … > file`, a relative
redirection while the shell stands in the checkout, and `git
reset/checkout/switch/restore/stash/clean` are now refused. `MultiEdit` is
registered in the hook matcher, the missing-`perl` fail-open now says so on
stderr, and the guard's `.log-to`/`.log-raw` switches are gitignored, absent
from the tree, and turn `check-orchestration.sh` red while they exist —
`.log-raw` writes whole hook payloads to disk, which invariant 7 forbids
near the repository.

`scripts/check-orchestration.sh` grew 20 cases for exactly these holes,
including the cases that must still pass: an agent doing all of it, and the
orchestrator reading and writing in its scratchpad.

## Not done, and why

- **V2, V3a, V3b, V4, V6, V10 were not run.** Their briefs are written and
  the V4/V6 fixtures were built, but the series costs nested-session quota
  on this account, and the request to run it under the server's second
  Claude account arrived through an agent relay rather than from Alexa in
  session. Using another account's stored credentials is not a session's own
  call, so neither half was done: no account switch, and no further quota
  spent. ORCH-005 asks Alexa; ORCH-032 holds the work.
- The matrix rows reading `live V2`, `live V4`, `live V10` are therefore
  still unverified live; they remain verified synthetically.
- The V4 and V6 fixture worktrees and their branches (`v4-x`, `v4-y`,
  `v6-task`) were **removed** rather than left behind, so the real
  repository keeps no test refs. The recipe to rebuild them is in the
  resume point.

## Gates

`./scripts/check-orchestration.sh`: **79 passed, 0 failed.**

`./scripts/check-docs.sh`:

```
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
note: D172 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```

The Rust gates were **NOT RUN** locally: this change touches only `docs/`,
`.claude/` and `scripts/`, and CI runs all four on the PR. They are not
reported as passed. No skip to name, since no suite ran.

## Risks and findings

- The guard remains best effort and is labelled so. `git -c x commit`,
  `/usr/bin/git commit`, `sh -c`, `python -c` and a redirection whose
  target starts with a shell variable still pass; they are in the matrix as
  documented bypasses with their own tests.
- The whole guard rests on "no `agent_id` and no `agent_type` means the
  orchestrator", observed on 2.1.303 only. A payload change would disable
  it silently, and it fails open by design. The contract and the review of
  traces stay the real protection.
- The guard cannot distinguish one role from another: every agent is exempt.
  Only the role file and the brief bound an agent, which is what ORCH-031
  is about.
- The suite looked flaky to the V7 reviewer (57/2 then 56/3). It was not:
  V11's agent was concurrently staging a line in `docs/status.md` in the
  shared worktree. Running a validation session that writes in the same
  worktree as the suite under test invalidates the suite's output.

## Decisions needed

ORCH-005 — which account runs the remaining nested validation sessions?
Recommendation: Alexa says so in session; an agent relay is not enough to
spend another account's credentials. Fallback: leave V2/V3/V4/V6/V10 unrun
and rely on the synthetic suite plus V1, V7 and V11.

## Resume point

Once ORCH-005 is answered, rewrite the five briefs from
`docs/orchestration/templates/brief.md` (the H7 copies lived in a scratchpad
that does not survive the session) and run them. The two fixtures:

```sh
# V4: two branches that change the same sentence
git worktree add -b v4-x <dir>/v4x orchestration-remediation
git worktree add -b v4-y <dir>/v4y orchestration-remediation
# edit the first sentence of docs/orchestration/gotchas/README.md
# differently in each, commit in each
# V6: a task interrupted between its file and its index row
git worktree add -b v6-task <dir>/v6 orchestration-remediation
# add a gotcha file, commit it WITHOUT its README.md row, then write a
# handoff whose Resume point is to add that row
```

Run each session in a worktree of its own, never the one the suite under
test is read from. Then fill the matrix rows `live V2`, `live V4`,
`live V10` in `docs/orchestration/permissions.md` and close ORCH-032.

## Processes

Four `claude -p` children, launched in the background from one Bash call and
all exited on their own (`exit=0` in their output files); no PID was left
running, and nothing was killed by name.
