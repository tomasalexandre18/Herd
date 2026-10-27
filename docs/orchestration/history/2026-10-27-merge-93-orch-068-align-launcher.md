# merge 93 orch-068-align-launcher — handoff 2026-10-27

- **Item:** ORCH-068
- **Agent:** herd-merger
- **Branch / commit:** `orch-068-align-launcher` / merge commit of this
  integration, based on origin/main `2ca6dc7`
- **PR:** #93

## Done

`origin/main` was merged into the branch. Only one file conflicted,
`docs/orchestration/board.md`, and the resolution is below. The branch
carries no Rust and no Cargo file: after the merge, `git diff
origin/main HEAD -- '*.rs' 'Cargo.toml' 'Cargo.lock' '*/Cargo.toml'` is
**empty**, so every Rust file in this tree is byte for byte
`origin/main`'s.

### The conflict, and how it was settled

**`docs/orchestration/board.md`, the `ORCH-068` row — one row, two
readings, neither clobbered.**

- **The branch** had the row where it stood on `cd59ef5`, in
  `## Blocked`, rewritten to `done` with the completion report.
- **`origin/main`** — PR #85's rewrite of the whole file — had already
  **moved the row out of `## Blocked`** into `## Owed`, state `owed`,
  owner `herd-step`, with a fuller statement of the defect (it is the
  version that spells out that a session started by
  `ops/agents/session.sh` can neither open nor merge a PR).
- **Kept:** `origin/main`'s placement in `## Owed` and its statement of
  the defect, into which the branch's completion report, its owner
  `herd-step` and its evidence list were carried; state `done`. The
  board's own convention decided the placement: a finished row stays in
  the section it was agreed in and changes state — `ORCH-031`,
  `ORCH-071`, `ORCH-073`, `ORCH-074` are all `done` inside `## Owed` or
  `## Open` on `origin/main`.
- **Added by the integrator to the kept statement**, because the
  orchestrator's brief says the defect is worse than the row reported:
  the missing `GH_CONFIG_DIR` also cost the session `git push`, since
  `origin` is https and the credential helper is `!gh auth
  git-credential` reading the same directory.
- **Origin cell:** both readings merged — "observed by the integrator of
  PR #72, 2026-10-26; answered by Alexa 2026-10-27, relayed by the
  orchestrator".
- The branch's copy of the row in `## Blocked` was **removed**. It had
  to be: `scripts/check-docs.sh` refuses the same id twice, and both
  copies could not stand.

No other hunk conflicted. Nothing of `origin/main` was dropped and
nothing of the branch was dropped.

### Board, beyond the row

`## Next` followed the row, because it still described ORCH-068 as work
to do:

- item 1 now reads "the row is `done`, PR #93, repository side only"
  where it read "the row is `owed` to `herd-step`";
- item 5 no longer lists ORCH-068 among "the owed rows that nothing
  blocks".

`docs/status.md` was **not** touched: it is 90 lines, at its limit, it
describes the state of the software, and no software state changed —
this branch is `CLAUDE.md`, `docs/`, `ops/` and `scripts/` only.

### One addition made at integration, and it was not reviewed with the rest

On the orchestrator's instruction, one line was added to
`docs/orchestration/templates/handoff.md`, in `## Processes`, the last
section an agent fills:

```
<own `target/` deleted once the branch was pushed, with the path; why
not, if not — the reason is in workflow.md "Concurrency and host rules">
```

One line, one file. The author of PR #93 offered the placement; the
orchestrator accepted it; `herd-merger` wrote it at integration. It
therefore did **not** go through the review the rest of the branch had,
and the PR description says so.

### `docs/decisions.md`

**Untouched, and no `D` number was allocated.** The branch carried no
`D-NEXT`; ORCH-068's decision is Alexa's of 2026-10-27, recorded on the
board row and in `CLAUDE.md` / `docs/orchestration/workflow.md`. The
`D170` gap stays a gap (D148).

## Not done, and why

- **The live launcher is still the live launcher.** Nothing under
  `/mnt/ssd_980/herd/claude/` was written, created or removed by this
  integration, and `.credentials.json` was never opened. Making the live
  file converge on the shipped one is the owner's act, and ORCH-068's
  row now says so.
- **ORCH-066 and ORCH-067** were not touched; they are their own rows.
- **No row was opened for ORCH-092**, which is in flight with no board
  row. It is not this branch's and its agent is still working.

## Gates

Run on the **merged** tree, in the integrator's own worktree.

```text
$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok

$ ./scripts/check-orchestration.sh
== ops/agents: the launcher gives a session what it needs (ORCH-068)
ok    env.sh keeps its state root on one rewritable line
ok    env.sh: HOME=<sandbox>/stateroot/claude/home
ok    env.sh: CLAUDE_CONFIG_DIR=<sandbox>/stateroot/claude/config
ok    env.sh: XDG_CONFIG_HOME=<sandbox>/stateroot/claude/home/.config
ok    env.sh: XDG_RUNTIME_DIR=<sandbox>/stateroot/claude/run
ok    env.sh: TMPDIR=<sandbox>/stateroot/claude/tmp
ok    env.sh: TEMP=<sandbox>/stateroot/claude/tmp
ok    env.sh: HISTFILE=<sandbox>/stateroot/claude/home/.bash_history
ok    env.sh: CARGO_HOME=<sandbox>/stateroot/claude/cargo
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

$ nix develop --command cargo fmt --all --check
(exit 0, no output)

$ nix develop --command cargo build --workspace
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 14s

$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 18.99s

$ nix develop --command ./scripts/test.sh
error: 5 targets failed:
    `-p herd-hub --test admission`
    `-p herd-hub --test killed_hub`
    `-p herd-hub --test owner_control`
    `-p herd-hub --test slots`
    `-p herd-runner --test busy_probe`

==============================================================
 AT LEAST 14 assertion(s) COULD NOT RUN on this host and were
 skipped, and the suite did not finish (cargo test exited 101),
 so this list is a floor and not a count.
==============================================================

test suite: FAILED (cargo test exited 101)
```

**The suite is red on this workstation and it is this host, not this
branch — and it is redder today than ORCH-069 describes.** Re-run alone,
target by target, with `--test-threads=1`, the five give **21 failures**:
8 in `killed_hub`, 4 in `owner_control`, 3 in `slots`, 4 in `admission`,
2 in `busy_probe`. **Every one of the 19 hub failures is a timeout, not a
mismatched value** — `real_herd/mod.rs:649` (`herdctl … never came back`,
the 30 s `PATIENCE`) or `real_herd/mod.rs:789` (`wait_until` past the same
deadline). The host carried five agents and, for part of the run, three
full suites at once, load between 2 and 19.

**Why this cannot be the branch, by construction:** after the merge,

```text
$ git diff origin/main HEAD -- '*.rs' 'Cargo.toml' 'Cargo.lock' '*/Cargo.toml'
(no output)
```

Every Rust and Cargo file in this tree is byte for byte `origin/main`'s.
The test binaries are built from `origin/main`'s sources, so whatever is
red here is red on `origin/main` on this host. **CI is the authority on
this branch.**

**Nothing is claimed about `slots.rs:744`.** Its assertion was never
reached: `the_derived_j_is_recorded_and_is_what_the_job_is_given` timed
out at `real_herd/mod.rs:649` before it. On CI that assertion is
**skipped** (`MemTotal` 15983 MiB under the guard's 17408), and by D27 a
skip is never a pass. The skip count above, **14**, is a **floor and not
a count** (D179): the suite did not finish.

## Risks and findings

1. **Three full suites were running on this host at the same time** —
   ORCH-079's, ORCH-094's and this one — with the load average between
   9 and 19. Timing-sensitive assertions are less reliable under that
   than they would be alone; anything red here beyond the two known
   ORCH-069 assertions should be read as the host under load and
   re-checked, not as a finding about the tree. CI's `check` is the
   authority on this branch, and it is green.
2. **`PR #92` will conflict with this file next.** It touches
   `docs/orchestration/board.md` and
   `docs/remediation/README.md`; its agent is still working. Whoever
   integrates it rebases onto this merge.
3. **Nothing in this branch can affect the running sessions.** `herd.sh`
   and `start-herd.sh` are not in this repository and still source the
   live `env.sh`.

## Decisions needed

None.

## Resume point

PR #94 (ORCH-093, `docs/disk-policy.md`), which is the second of the two
integrations this merger holds, and which needs the board rows ORCH-093
to ORCH-104 written from that document's §7 plus one row for its §6
decision A.

## Processes

Started and ended by this integration: one `scripts/check-orchestration.sh`
(pid 1312206), one `cargo build` / `cargo clippy` pair, one
`scripts/test.sh` (pid 2025499). No process of another agent was signalled;
no `pkill`, no `killall`.
<own `target/` deleted once the branch was pushed — recorded below once
it is; the reason is in workflow.md "Concurrency and host rules">
