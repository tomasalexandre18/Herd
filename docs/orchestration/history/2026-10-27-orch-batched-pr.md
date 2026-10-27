# orch-batched-pr — handoff 2026-10-27

- **Item:** none allocated (see *Decisions needed*; the board was being
  written by `herd-merger` for the #92 + #98 batch and was left alone)
- **Agent:** herd-step
- **Branch / commit:** `orch-batched-pr` / `2a59185`, based on
  origin/main `f19efe6`. This line naming the sha is the one change in
  the small commit that follows it.
- **PR:** none — the branch is the delivery (one PR per batch,
  `docs/orchestration/workflow.md`), which is the rule this branch writes

## Done

Alexa's change of method of 2026-10-27, *"pour le moment on applique
uniquement le une PR = plusieur lot, le reste non"*, is written down in
one normative place plus one reasoned place, as ORCH-068 did for the
concurrency rule.

- `CLAUDE.md` "Git and main": the one-PR-per-batch bullet replaces the
  bullet that said every change to `main` goes through a pull request
  filled in from the template. It states that an agent pushes its branch
  and opens no PR, that `herd-merger` merges the batch one branch at a
  time onto one integration branch and opens **one** PR from the
  template with Evidence per branch, and points at
  `docs/orchestration/workflow.md` for the reasoning and the refusals.
  `never --admin` was added to the CI bullet.
- `docs/orchestration/workflow.md`: a new section *One PR per batch of
  branches* with Alexa's words verbatim, what is adopted (including
  why one branch at a time, that the batch is everything ready when the
  merger starts and that this is the orchestrator's call, that the
  Evidence block carries each branch's evidence separately, and that
  superseded single PRs are closed naming the batch PR), what is
  unchanged (CI the gate, ORCH-001, never `--no-verify`, never
  force-push, never a red merge, never `--admin`, one integration at a
  time — a batch is one integration, larger), and the three refusals
  with their reason: testing only what was touched; making this
  workstation's local run the gate (ORCH-069, ORCH-107, skips per host,
  reconsiderable once those two are fixed); a nightly full pass, named
  as not adopted *and* as the missing piece.
- `docs/orchestration/workflow.md` also: the roles table (`herd-step`
  never opens a PR, `herd-merger` integrates a batch, `herd-release`
  never opens its own PR), loop step 4 (the agent opens no PR), step 7
  (rewritten for the batch), step 8 (`herd-release` pushes
  `cut-v<version>`, `herd-merger` opens its PR) and the concurrency
  bullet (the board shows which **batch** the merger holds).
- `.github/pull_request_template.md`: new section *Branches in this
  batch* (branch, `ORCH-nnn`, handoff path, in merge order; a single
  branch is a batch of one); the Evidence section now asks for one block
  per branch, named for it; the Handoff section no longer asks for "the
  task's" single handoff.
- `docs/orchestration/templates/handoff.md`: the `- **PR:**` line now
  says none, an agent opens none, and that `herd-merger` names the batch
  PR there.
- `.claude/agents/herd-merger.md`: description and steps 1, 2, 3, 5, 6
  and the handoff path rewritten for a batch merged one branch at a
  time, with the attributability reason stated once.
- `.claude/agents/herd-step.md`: the existing "**No PR**" line keeps its
  wording and gains the pointer (one PR per batch, opened by
  `herd-merger`).
- `.claude/agents/herd-release.md`: "No PR merge either" becomes "No
  pull request either: you push `cut-v<version>` and hand off".

What was checked and needed no change:

- `scripts/check-docs.sh`: asserts nothing about one PR per task and
  nothing about a handoff naming a PR. Its only PR mention is the note
  that a `D` gap may be "held by an open PR", which a batch PR holds in
  exactly the same way.
- `scripts/check-orchestration.sh`: asserts nothing about one PR per
  task either. Its two PR lines are `gh pr merge 70 --squash` must be
  blocked for the orchestrator and `gh pr view 70; gh pr checks 70` must
  be allowed; both stay true.
- `.claude/agents/herd-planner.md` and `herd-reviewer.md`: neither opens
  a PR; `gh pr view` is read-only. Untouched.
- `docs/orchestration/permissions.md` line 42 already records that
  `herd-step` is refused a PR; `gotchas/docs-only-gates.md` says the
  PR's Evidence block shows the `check-docs.sh` output, which the batch
  PR does, per branch. Untouched.

## Not done, and why

- No `docs/decisions.md` entry and no `D-NEXT`: the brief forbids it;
  only `herd-merger` appends.
- No board row: `herd-merger` holds `docs/orchestration/board.md` for
  the #92 + #98 batch. Writing a row would have collided. The row this
  change deserves is listed under *Decisions needed*.
- No `herd-reviewer` pass. `workflow.md` loop step 6 says a change to
  `CLAUDE.md` or to an agent gets one before integration; this branch is
  both. Owed before the batch PR merges.

## Gates

Prose and configuration only. The four Rust gates were **NOT RUN**
locally; CI runs them on the batch PR this branch will join.

`./scripts/check-docs.sh`:

```text
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
exit=0
```

`./scripts/check-orchestration.sh`:

```text
== check-docs.sh goes green on the repository as it is
ok    unmodified copy passes
== check-docs.sh goes red on each defect it claims to catch
ok    duplicate decision number D27
ok    D-NEXT placeholder
ok    malformed decision header
ok    CLAUDE.md saying the orchestrator works alone
ok    CLAUDE.md no longer pointing at the board
ok    a missing role file
ok    a role whose name does not match its file
ok    board sections out of shape
ok    board row with missing cells
ok    board row with an unknown state
ok    board id used twice
ok    board missing
ok    undated handoff
ok    a brief naming no worktree (the Write only boilerplate does not exempt it)
ok    a brief naming the orchestrator's checkout as its worktree
ok    a brief whose only path is inside a git worktree add command
ok    a brief naming an absolute worktree of its own
ok    a read-only brief, which writes nothing in the repository
ok    a handoff whose task name merely contains the word brief
ok    over-long line in docs/orchestration
ok    hook not executable
ok    settings.json invalid
== the number is an identifier: the historical gaps stay silent, a new gap is noted, nothing is rewritten
ok    known gaps D143, D154 not noted
ok    decisions.md unchanged by the check
== two branches allocating the same number collide at integration, and the check says so
ok    branch A allocated D4 and passes
ok    the collision D4/D4 is rejected with the number named
ok    B reallocated to D5 passes
== orchestrator-guard.sh: blocks the main session's writes, lets agents and inspection through
ok    guard exit 2: {"tool_name":"Edit","cwd":"<repo>","tool_input":{"file_path":"<repo>/docs/status.md"}}
ok    guard exit 2: {"tool_name":"Write","cwd":"<repo>","tool_input":{"file_path":"docs/status.md"}}
ok    guard exit 0: {"tool_name":"Edit","cwd":"<repo>","agent_id":"abc","tool_input":{"file_path":"<repo>/docs
ok    guard exit 0: {"tool_name":"Edit","cwd":"<repo>","agent_type":"herd-step","tool_input":{"file_path":"<re
ok    guard exit 0: {"tool_name":"Bash","cwd":"<repo>","agent_type":"herd-merger","tool_input":{"command":"git
ok    guard exit 0: {"tool_name":"Write","cwd":"<repo>","tool_input":{"file_path":"/tmp/claude-1002/x/brief.md
ok    guard exit 0: {"tool_name":"Edit","cwd":"<repo>","tool_input":{"file_path":"<repo>/.claude/worktrees/age
ok    guard exit 2: {"tool_name":"Bash","cwd":"<repo>","tool_input":{"command":"cd x && git commit -m hi"}}
ok    guard exit 2: {"tool_name":"Bash","cwd":"<repo>","tool_input":{"command":"gh pr merge 70 --squash"}}
ok    guard exit 2: {"tool_name":"Bash","cwd":"<repo>","tool_input":{"command":"git merge --no-ff feature"}}
ok    guard exit 2: {"tool_name":"Bash","cwd":"<repo>","tool_input":{"command":"sed -i 's/a/b/' docs/status.md
ok    guard exit 2: {"tool_name":"Bash","cwd":"<repo>","tool_input":{"command":"cat > docs/status.md <<EOF\nx\
ok    guard exit 2: {"tool_name":"Bash","cwd":"<repo>","tool_input":{"command":"git tag -a v1 -m x"}}
ok    guard exit 2: {"tool_name":"Bash","cwd":"<repo>","tool_input":{"command":"gh release create v1"}}
ok    guard exit 2: {"tool_name":"Bash","cwd":"<repo>","tool_input":{"command":"git push origin main"}}
ok    guard exit 2: {"tool_name":"Bash","cwd":"<repo>","tool_input":{"command":"echo x > <repo>/CLAUDE.md"}}
ok    guard exit 0: {"tool_name":"Bash","cwd":"<repo>","tool_input":{"command":"git log --oneline -5; git diff
ok    guard exit 0: {"tool_name":"Bash","cwd":"<repo>","tool_input":{"command":"gh pr view 70; gh pr checks 70
ok    guard exit 0: {"tool_name":"Bash","cwd":"<repo>","tool_input":{"command":"cat > /tmp/claude-1002/x/brief
ok    guard exit 0: {"tool_name":"Bash","cwd":"<repo>","tool_input":{"command":"grep -n 'git commit' docs/deci
ok    guard exit 0: {"tool_name":"Bash","cwd":"<repo>","tool_input":{"command":"git worktree add -b t /tmp/cla
ok    guard exit 0: {"tool_name":"Bash","cwd":"<repo>","tool_input":{"command":"./scripts/check-docs.sh; ls <r
ok    guard exit 0: {"tool_name":"Bash","cwd":"<repo>","agent_id":"a1","tool_input":{"command":"git commit -m 
ok    guard exit 0: {"tool_name":"Bash","cwd":"<repo>","agent_type":"herd-step","tool_input":{"command":"git c
ok    guard exit 0: {"tool_name":"Edit","cwd":"<repo>","agent_type":"herd-merger","tool_input":{"file_path":"<
ok    guard exit 0: not json at all
ok    guard exit 0: {"tool_name":"Bash","cwd":"<repo>","tool_input":{"command":"git commit -m hi"}}
== the holes the V7 review found (ORCH-004), now closed
ok    guard exit 2: {"tool_name":"Edit","cwd":"<repo>","tool_input":{"file_path":"<repo>/.claude/worktrees/../
ok    guard exit 2: {"tool_name":"Write","cwd":"<repo>","tool_input":{"file_path":"/tmp/..<repo>/docs/x.md"}}
ok    guard exit 2: {"tool_name":"Edit","cwd":"<repo>/docs","tool_input":{"file_path":"../CLAUDE.md"}}
ok    guard exit 0: {"tool_name":"Edit","cwd":"<repo>","tool_input":{"file_path":"<repo>/.claude/worktrees/a-1
ok    guard exit 0: {"tool_name":"Edit","cwd":"<repo>","tool_input":{"file_path":"<repo>/.claude/worktrees/a-1
ok    guard exit 2: {"tool_name":"Bash","cwd":"<repo>","tool_input":{"command":"echo x > a.md"}}
ok    guard exit 2: {"tool_name":"Bash","cwd":"<repo>","tool_input":{"command":"cd docs && echo x > a.md"}}
ok    guard exit 2: {"tool_name":"Bash","cwd":"/tmp/claude-1002/x","tool_input":{"command":"cd <repo>/docs && 
ok    guard exit 2: {"tool_name":"Bash","cwd":"<repo>","tool_input":{"command":"git reset --hard"}}
ok    guard exit 2: {"tool_name":"Bash","cwd":"<repo>","tool_input":{"command":"git checkout -- docs/status.md
ok    guard exit 2: {"tool_name":"Bash","cwd":"<repo>","tool_input":{"command":"git stash"}}
ok    guard exit 2: {"tool_name":"Bash","cwd":"<repo>","tool_input":{"command":"git clean -fd"}}
ok    guard exit 0: {"tool_name":"Bash","cwd":"<repo>","agent_id":"a1","tool_input":{"command":"cd docs && ech
ok    guard exit 0: {"tool_name":"Bash","cwd":"<repo>","agent_type":"herd-merger","tool_input":{"command":"git
ok    guard exit 0: {"tool_name":"Bash","cwd":"<repo>","tool_input":{"command":"git log -1 > /tmp/claude-1002/
ok    guard exit 0: {"tool_name":"Bash","cwd":"<repo>","tool_input":{"command":"./scripts/check-docs.sh 2>&1 |
ok    guard exit 0: {"tool_name":"Bash","cwd":"<repo>","tool_input":{"command":"wc -l docs/status.md >/dev/nul
ok    perl is present, the guard can parse
ok    no logging switch left in .claude/hooks
ok    the logging switches are gitignored
== known bypasses, documented in docs/orchestration/permissions.md (these pass on purpose)
ok    guard exit 0: {"tool_name":"Bash","cwd":"<repo>","tool_input":{"command":"git -c a=b commit -m hi"}}
ok    guard exit 0: {"tool_name":"Bash","cwd":"<repo>","tool_input":{"command":"/usr/bin/git commit -m hi"}}
== release-guard.sh
ok    release exit 2: git tag -a v0.4.0 -m x
ok    release exit 2: git push origin v0.4.0
ok    release exit 2: git push --tags
ok    release exit 2: gh release create v0.4.0
ok    release exit 2: gh workflow run release.yml
ok    release exit 2: docker load < hub.tar
ok    release exit 2: sudo systemctl restart herd-runner
ok    release exit 0: git tag -l
ok    release exit 0: git tag --list 'v*'
ok    release exit 0: git tag -n9 v0.3.7
ok    release exit 0: git tag
ok    release exit 2: git tag -l; git tag -a v1 -m x
ok    release exit 2: git tag v0.4.0
ok    release exit 0: git push origin cut-v0.4.0
ok    release exit 0: gh run list --workflow release.yml
ok    release exit 0: git log v0.3.7..origin/main --oneline
== the skip report never claims more than it saw (ORCH-071, D27)
ok    test.sh: red with an empty skip log reports it unknown
ok    test.sh: red with skips reports them as a floor
ok    test.sh: the floor lists the skips it has
ok    test.sh: green with an empty skip log says nothing was skipped
ok    test.sh: green with skips reports the real count
ok    test.sh: a finished run prints the marker report-skips.sh reads
ok    test.sh: an unfinished run prints the failed marker
ok    test.sh: a narrowed green run does not speak for the whole suite
ok    test.sh: a narrowed green run says its count may be short
ok    test.sh: a narrowed run's marker is not a plain PASSED
ok    test.sh: --no-fail-fast is passed by default
ok    test.sh: the caller's own --no-fail-fast is not doubled
ok    report-skips.sh: a red run with no skip line reports it unknown
ok    report-skips.sh: a red run with skips reports them as a floor
ok    report-skips.sh: a finished run with no skips says nothing was skipped
ok    report-skips.sh: a finished run reports the real count
ok    report-skips.sh: a narrowed run is not read as a whole one
ok    report-skips.sh: an output with no verdict marker is not a pass
== ops/agents: the launcher gives a session what it needs (ORCH-068)
ok    env.sh keeps its state root on one rewritable line
ok    env.sh: HOME=/mnt/ssd_980/herd/claude/tmp/herd-orch-check.X5kpDu/stateroot/claude/home
ok    env.sh: CLAUDE_CONFIG_DIR=/mnt/ssd_980/herd/claude/tmp/herd-orch-check.X5kpDu/stateroot/claude/config
ok    env.sh: XDG_CONFIG_HOME=/mnt/ssd_980/herd/claude/tmp/herd-orch-check.X5kpDu/stateroot/claude/home/.config
ok    env.sh: XDG_RUNTIME_DIR=/mnt/ssd_980/herd/claude/tmp/herd-orch-check.X5kpDu/stateroot/claude/run
ok    env.sh: TMPDIR=/mnt/ssd_980/herd/claude/tmp/herd-orch-check.X5kpDu/stateroot/claude/tmp
ok    env.sh: TEMP=/mnt/ssd_980/herd/claude/tmp/herd-orch-check.X5kpDu/stateroot/claude/tmp
ok    env.sh: HISTFILE=/mnt/ssd_980/herd/claude/tmp/herd-orch-check.X5kpDu/stateroot/claude/home/.bash_history
ok    env.sh: CARGO_HOME=/mnt/ssd_980/herd/claude/tmp/herd-orch-check.X5kpDu/stateroot/claude/cargo
ok    env.sh: ANTHROPIC_API_KEY=<unset>
ok    env.sh: ANTHROPIC_BASE_URL=<unset>
ok    env.sh: CHATONS_ROOT=<unset>
ok    env.sh: HC=<unset>
ok    env.sh creates home
ok    env.sh creates config
ok    env.sh creates run
ok    env.sh creates tmp
ok    env.sh creates cargo
ok    GH_CONFIG_DIR is an absolute gh directory outside the state root
ok    HERD_GH_CONFIG_DIR overrides the default
ok    PATH puts dtach, the wrappers, the per-user profile and sw/bin in front
ok    PATH keeps what it inherited behind the four
ok    the guard refuses a state path outside the root
ok    session.sh starts the session and says nothing when gh is logged in
ok    session.sh warns, and still starts, when gh has no login
ok    session.sh refuses without credentials when no terminal can log in
ok    ops/agents names paths only, never a token (invariant 7)

check-orchestration: 138 passed, 0 failed
exit=0
```

No skip: neither script prints a `HERD-SKIPPED` line.

## Risks and findings

- **One thing had to leave `CLAUDE.md`, and it is named here.**
  `CLAUDE.md` was at exactly 150 lines on `f19efe6`, its enforced limit,
  so the new rule had to pay for itself. It cost four lines against the
  two the old bullet used; the other two came from rewrapping two
  paragraphs ("Gates before a commit" and the "Session start" paragraph)
  at 79 columns without removing a word. What did **not** fit is the
  explicit path `.github/pull_request_template.md`, which the old bullet
  named and the new one shortens to "the template"; the path stays in
  `workflow.md` step 7 and in `herd-merger.md` step 6. Nothing else was
  cut, and no invariant was touched: invariant 8 could also be rewrapped
  one line shorter, but reflowing invariant text is exactly what the
  gotcha `invariants-are-quoted-not-reflowed` warns against, so it was
  left alone and is reported here instead.
- **ORCH-069 and ORCH-107 are not on `main`'s board.** `workflow.md` now
  names them as the two fixes that make local-gate reconsiderable, but
  `docs/orchestration/board.md` at `f19efe6` lists only up to ORCH-032;
  those ids live on branches not yet merged. A reader who follows them
  today finds nothing. They become findable when the batch carrying
  those rows lands; no action needed on this branch.
- **This branch changes `CLAUDE.md` and three agent files**, so loop
  step 6 owes it a `herd-reviewer` pass before the batch PR merges.
- **Rebase expected.** `herd-merger` holds `board.md` for the #92 + #98
  batch; this branch does not touch that file, but `workflow.md` and
  `CLAUDE.md` are popular and a conflict there is plausible.
- The new rule applied to this task: the branch is pushed and **no pull
  request was opened**.

## Decisions needed

- A board row for this change (the brief left it to judgement and the
  board was held by another agent). Recommendation: `herd-merger` or the
  next agent that writes `board.md` adds one `done` row under *Owed* or
  the batch's section — item "One PR per batch of branches (Alexa,
  2026-10-27) written into `CLAUDE.md` and `workflow.md`", owner
  `herd-step`, evidence this handoff. Fallback: no row, and the decision
  lives only in `workflow.md` — findable, but not on the one list.
- A nightly full pass is recorded in `workflow.md` as **missing**, not
  merely refused. Whether to open an item for it is Alexa's call on
  *what* gets built. Recommendation: open it as `open` on the board, not
  as work. Fallback: leave the sentence in `workflow.md` as the record.

## Resume point

`herd-merger` picks `orch-batched-pr` up in its next batch (a
`herd-reviewer` pass first, per loop step 6). Nothing is left half-done
on the branch.

## Processes

Two foreground-then-background runs of `./scripts/check-orchestration.sh`
started by this agent and exited on their own (shell task ids
`bdyox31wb`, `bamvlv58m`); one `cat` that waited on stdin by mistake was
stopped with the harness's own task stop (`b4p869sj0`). No `kill` by
name, no `pkill`. No `target/` was created: no Rust gate was run.
