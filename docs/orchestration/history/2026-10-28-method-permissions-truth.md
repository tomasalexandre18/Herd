# method-permissions-truth — handoff 2026-10-28

- **Item:** the last three gaps in the method transition (no ORCH number
  allocated to this task; it closes findings three agents reported)
- **Agent:** herd-step
- **Branch / commit:** `method-permissions-truth` / see below, rebased
  onto `origin/method-push-guard` `1f9edf1` (not `origin/main`). It was
  branched from `7cd0c6f`; the base moved mid-task and the rebase applied
  with no conflict, including in `scripts/check-orchestration.sh`, the one
  file both branches touch.
- **PR:** none — an agent pushes its branch and opens none (one PR per
  batch, `docs/orchestration/workflow.md`); `herd-merger` names it here

## Done

**1. `git -c key=val` is closed in `.claude/hooks/orchestrator-guard.sh`.**
`-c` joined the strip list that normalises a git command before the git
rules run, so the verb is matched wherever a global option puts it.
`--config-env=key=var` joined it in the same move: it is the same bypass
in a second spelling, and closing one while leaving the other open would
have made the sentence this task was asked to write untrue. The header
and the inline comment now record that the two config forms were a
documented, asserted bypass until 2026-10-28 and why they were closed —
the guard stopped being a convenience in front of a required status check
and became one of only two mechanisms in front of `main`.

`/usr/bin/git commit` is untouched and still documented as open, in the
hook header, in `check-orchestration.sh` and in `permissions.md`: it is a
path and not an option, and a rule on every absolute path to every git on
the host buys less than it costs.

Measured on the real hook, before the cases were written:

```
exit=2  git -c a=b commit -m hi
exit=2  git -c a=b push origin main
exit=2  git -C /x -c a=b rebase origin/main
exit=2  git -c core.hooksPath= push origin main
exit=2  git --config-env=a=B commit -m hi
exit=2  git --config-env a=B push origin main
exit=2  git -c a=b -c c=d merge origin/main
exit=0  git -c color.ui=false log --oneline -3
exit=0  git -c a=b status --short
exit=0  /usr/bin/git commit -m hi
```

Nothing unexpected broke. Reading through `-c` still passes, which was
the thing worth checking: a guard that refuses the orchestrator a
`git -c color.ui=false log` is a guard somebody switches off.

**2. `docs/orchestration/permissions.md` is corrected.** The file keeps
its shape: the two opening sections, the matrix, the disabling notes, the
live-verification table. What changed:

- The opening paragraph no longer credits the project with CI, the PR
  template's Evidence and branch protection as gates.
- A new section, *What gates `main`, and what protects nothing*, quotes
  the `gh api` output measured today (re-read by me, not taken from the
  brief) and states plainly that `enforce_admins: false` plus an admin
  credential means no branch-protection rule binds us, that
  `allow_force_pushes: false` and `allow_deletions: false` therefore
  protect nothing against our own credential, and that
  `required_status_checks` gates a merge button nobody presses. It names
  `scripts/gate.sh`, `scripts/githooks/pre-push` and the landing record
  as what stands in front of `main`, the record's permanence being
  contractual rather than kept by a server, and `--no-verify` as the
  hole in the hook.
- A sub-section documents `core.hooksPath` as one repository-wide
  setting, named as an exception rather than a per-worktree duty, with
  what it does not give us (see item 3).
- The `pre-push` bullet names the gated commit as the **second parent**
  of the `--no-ff` merge, following the base branch's `1f9edf1`, and adds
  the consequence that branch asked for: the hook refuses a landing whose
  merge changed the tree, `main` moving under the merge produces exactly
  that, so one merger at a time is load-bearing for the guard rather than
  good manners. `permissions.md` is the right place for the
  *consequence*, because the file is about what enforces what; the
  *instruction* stays in `docs/orchestration/workflow.md`, which another
  branch owns and which this diff does not touch.
- The known-bypass row became two rows: the global-option forms, now
  closed, and what is still open.
- The three rows that attributed enforcement to branch protection —
  `herd-step` on `main`/PR/tag/release, `herd-merger` on a red check or
  an empty Evidence block, and *anyone* on `--no-verify`, force-push and
  a red merge — now name `scripts/gate.sh`, the pre-push hook and the
  contract, with `--no-verify` and `enforce_admins: false` named as the
  holes. The rules branch's claim that every sentence in this project
  asserting a server-side refusal of `main`'s history has been rewritten
  is true of this file as of this commit.
- The limits paragraph under the live-verification table now says that
  all of those runs predate the pull request leaving the loop, so where
  one shows `main` untouched it shows the contract being kept.

Evidence, in full, as the API returned it:

```
$ gh api repos/tomasalexandre18/herd/branches/main/protection
{"url":"https://api.github.com/repos/tomasalexandre18/Herd/branches/main/protection","required_status_checks":{"url":"https://api.github.com/repos/tomasalexandre18/Herd/branches/main/protection/required_status_checks","strict":false,"contexts":["check","docs"],"contexts_url":"https://api.github.com/repos/tomasalexandre18/Herd/branches/main/protection/required_status_checks/contexts","checks":[{"context":"check","app_id":15368},{"context":"docs","app_id":15368}]},"required_pull_request_reviews":{"url":"https://api.github.com/repos/tomasalexandre18/Herd/branches/main/protection/required_pull_request_reviews","dismiss_stale_reviews":false,"require_code_owner_reviews":false,"require_last_push_approval":false,"required_approving_review_count":0},"required_signatures":{"url":"https://api.github.com/repos/tomasalexandre18/Herd/branches/main/protection/required_signatures","enabled":false},"enforce_admins":{"url":"https://api.github.com/repos/tomasalexandre18/Herd/branches/main/protection/enforce_admins","enabled":false},"required_linear_history":{"enabled":false},"allow_force_pushes":{"enabled":false},"allow_deletions":{"enabled":false},"block_creations":{"enabled":false},"required_conversation_resolution":{"enabled":false},"lock_branch":{"enabled":false},"allow_fork_syncing":{"enabled":false}}

$ gh api repos/tomasalexandre18/herd --jq .permissions
{"admin":true,"maintain":true,"pull":true,"push":true,"triage":true}

$ gh api repos/tomasalexandre18/herd/rulesets
[]
$ gh api repos/tomasalexandre18/herd/rules/branches/main
[]
```

No GitHub setting was changed. Nothing was force-pushed or deleted to
test the exemption; the file says so in those words.

**3. `core.hooksPath` is set, and the premise was proved first.** Per the
mid-task correction, `/mnt/ssd_980/herd-project/.git/hooks` was listed
before anything was written. It holds 14 `*.sample` files and no active
hook:

```
$ find /mnt/ssd_980/herd-project/.git/hooks -maxdepth 1 -type f \
      ! -name '*.sample' -printf '%M %p\n'
(no output)
```

So the condition was met and the setting was made, once:

```
$ git -C /mnt/ssd_980/herd-project config core.hooksPath scripts/githooks
rc=0
$ git -C /mnt/ssd_980/herd-project config --list --show-origin \
    | grep -i hookspath
file:.git/config	core.hookspath=scripts/githooks

$ cd <this worktree> && ./scripts/githooks/pre-push --check-install
pre-push: installed — core.hooksPath=scripts/githooks
rc=0
```

It is **inert until this batch lands.** The orchestrator's checkout sits
on `main`, where `scripts/githooks/` does not exist:

```
$ ls -d /mnt/ssd_980/herd-project/scripts/githooks
ls: cannot access '…/scripts/githooks': No such file or directory
$ git -C /mnt/ssd_980/herd-project rev-parse --abbrev-ref HEAD
main
```

A `core.hooksPath` naming a missing directory runs nothing and takes
`.git/hooks` out of the loop as well, so between this commit and the
landing the checkout has no git hook at all. That costs nothing only
because the repository keeps none of its own, which is why it is written
down in `permissions.md` and asserted in `check-orchestration.sh` rather
than left as a remark.

`.git/config` in the orchestrator's checkout is the only thing I wrote
outside this worktree. No tracked file there was touched, no
`git worktree prune`, no other command.

## Not done, and why

- **No PR, no merge, no tag, no release, no write to `main`**: not this
  task's.
- **No GitHub setting changed.** Narrowing branch protection
  (`enforce_admins`, or removing rules that now protect nothing) is
  sequenced after the last pull request and is somebody else's act. The
  file describes today's settings, not the ones we want.
- **`--check-install` was not added to `scripts/gate.sh` step 0**, as
  instructed, and my answer to whether it belongs there is **no, not as
  a failing step** — see *Risks and findings*.
- **The four Rust gates were NOT RUN**, with the reason under *Gates*.
- `docs/orchestration/board.md`, `docs/decisions.md`, `CLAUDE.md`,
  `workflow.md`, `.claude/agents/`, `scripts/gate.sh` and
  `scripts/githooks/pre-push` are untouched: other branches own them.
  This task allocates no decision, so there is no `D-NEXT` entry.

## Gates

`./scripts/check-orchestration.sh`, last run after this handoff was
written, from the worktree:

```
check-orchestration: 243 passed, 0 failed
```

The base was re-measured here, not taken on trust: `origin/method-push-guard`
`1f9edf1` checked out in a detached worktree of its own gives

```
check-orchestration: 226 passed, 0 failed
```

The added cases are the config-option cases for item 1 and the
`core.hooksPath` cases for item 3, both inside the
`orchestrator-guard.sh` and `pre-push` sections the base branch created,
minus the two `og 0` assertions flipped to `og 2` and one `hook 0`
assertion that was deleted where it sat, in the "known bypasses" section,
and rewritten in the guard section so one place says what the guard does
with a global option. `og 2 'git -C /x -c a=b rebase origin/main'` was
kept and flipped, not deleted.

`./scripts/check-docs.sh`:

```
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```

The four Rust gates were **NOT RUN**, for two reasons: this diff touches
only a shell hook, a shell check script and a Markdown file, so it is
invisible to cargo; and the brief forbade a build because another agent
needs the machine. They are not reported as passed.

**Skips:** no skip. Neither check script reports a `HERD-SKIPPED` line,
and the Rust suite — which is where D27 skips live — was not run.

**One intermittent failure, not mine.** Twice the run came back with 16
failures, all sixteen downstream of `FAIL  PATH dropped the inherited
entries; the guard below needs coreutils` — the known ORCH-111 case near
line 464. Every `gate:` case after it fails as a consequence, so the
signature is unmistakable: one PATH failure followed by exactly the
fifteen gate cases. Re-running cleared it both times. Reporting it as
the brief asked: 2 occurrences in 8 runs on this host today, across the
base branch and mine.

## Risks and findings

**Where the brief was wrong, besides the correction already sent.**

1. The brief said `git config --worktree` "is refused because
   `extensions.worktreeConfig` is unset". Measured on git 2.54.0, that
   holds only in a repository with **more than one** working tree. In a
   single-worktree repository `--worktree` is accepted and silently
   means `--local`:

   ```
   $ git config --worktree core.hooksPath scripts/githooks   # 1 worktree
   rc=0
   $ git config --list --show-origin | grep -i hookspath
   file:.git/config	core.hookspath=scripts/githooks
   $ git worktree add -q ../wt -b w2 && git config --worktree \
       core.hooksPath scripts/githooks
   fatal: --worktree cannot be used with multiple working trees unless
   the config extension worktreeConfig is enabled. …
   rc=128
   ```

   My first draft of the case asserted the brief's version and turned
   the check red, which is how this surfaced. Both behaviours are now
   asserted. The conclusion is unchanged and stronger: `--worktree` is
   no way to narrow the setting by accident in *our* repository, which
   has 88 working trees.

2. `git worktree list | wc -l` in the orchestrator's checkout prints
   **88**, not the 85 the brief quoted — the count includes the main
   checkout, and three more worktrees may have been added since the
   reviewer measured. Nothing depends on the figure; `permissions.md`
   says 88 and whoever re-reads it should re-count.

**Is `--check-install` fit for `gate.sh` step 0?** No, not as a step
that can fail, and my reasons are measured rather than aesthetic:

- `scripts/gate.sh` is run on scratch repositories by
  `check-orchestration.sh`'s own `gate_fresh`, which copies `gate.sh`
  and stubs the other scripts but does **not** copy
  `scripts/githooks/pre-push` and does not set `core.hooksPath`. A
  failing step 0 would turn roughly twenty of the gate's own cases red
  and the fix would be to teach the harness to install a hook it is not
  testing.
- `--check-install` answers about the *config*, not about the
  directory: it reports "installed" in a worktree whose
  `scripts/githooks/` is absent, which is exactly the state item 3
  created. So as a gate step it would have passed in the one situation
  where the hook genuinely does not run.
- A gate that refuses to run is indistinguishable, to someone in a
  hurry, from a gate worth bypassing. The first false refusal is an
  argument for `--no-verify`, and the pre-push hook's own header makes
  that argument about itself.

  What *would* earn its place is a **non-fatal warning** — one line from
  `gate.sh` when `core.hooksPath` does not point at `scripts/githooks`,
  not affecting the verdict — or the same line in `workflow.md`'s
  worktree-creation step, which already owns the instruction. Both are
  somebody else's file, so neither is in this diff.

**A fresh clone carries no config at all.** `.git/config` is not in the
repository and never will be, so `core.hooksPath` is a duty on whoever
makes a working copy, not a property of the repository. Today that
means: one setting on this host covers every worktree added from
`/mnt/ssd_980/herd-project`, and a second machine, a CI checkout or a
re-clone after a disk failure has the guard **off** until someone runs
the command. Nothing warns them. CI never had the hook and never will
— `.github/workflows/ci.yml` runs `check-orchestration.sh` directly and
no workflow runs `gate.sh` — so there is no server anywhere in this
picture.

**What the closed bypass does not buy.** The guard constrains the main
session only; every execution agent carries `agent_id` or `agent_type`
and is let through before any rule runs. Closing `-c` makes the
orchestrator's accidental `git -c … commit` fail. It does nothing about
an agent, and the agent is who pushes. `scripts/githooks/pre-push` is
what stands in front of an agent, and `git -c core.hooksPath= push`
bypasses *that* — denied for the orchestrator by this change, open to
every agent by design.

**One of my own cases had to be rewritten after the rebase**, and it is
worth naming because it shows the base branch's change is real. The case
that proves the installed hook runs while a stray `.git/hooks/pre-push`
stays silent originally asserted the refusal message contained
`receipt`. On `1f9edf1` a push whose tip is not a `--no-ff` merge is
refused earlier, with "has no second parent", so the case failed. It now
asserts the *identity* of the hook that spoke — our `pre-push: REFUSED`
prefix, and not the stray's marker — which is what the case was about;
it no longer depends on which rule fires first.

**Pre-existing, unrelated, and not fixed here:** the guard denies
`git commit --help` (exit 2), because the write-verb rule matches
`git commit` whatever follows. It predates this change and reading a
man page is not a write; leaving it is a judgement, not an oversight.

## Decisions needed

None blocking. One worth a line when the branch-protection task is
briefed: with `enforce_admins: false`, the rules currently on `main`
(`allow_force_pushes: false`, `allow_deletions: false`,
`required_status_checks`) are decoration, so that task is either
"turn `enforce_admins` on and keep the rules" or "remove the rules and
say in one place that `main` is protected locally and by the contract".
`permissions.md` now describes the first state as the one we are in and
takes no position on which way to go.

## Resume point

```
git -C /mnt/ssd_980/herd-project fetch origin
git log --oneline origin/method-permissions-truth -1
```

For `herd-merger`: this branch is based on `origin/method-push-guard`,
not on `origin/main`, and must land in the same batch as it or after it —
`permissions.md` here names `scripts/gate.sh` and
`scripts/githooks/pre-push`, which arrive with that branch. The one
command of item 3 has already been run on this host and does not need
repeating; a reader on another machine needs it.

## Processes

No background process, no PID started, nothing killed. No cargo build,
no test run, so no `target/` was created in this worktree and none was
deleted. The worktree, registered until someone prunes it:

```
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/method-permissions/wt
```
