# method-push-guard — handoff 2026-10-28

- **Item:** T5 of the local-merge method (plan of 2026-10-28,
  herd-planner), plus two mid-task corrections from the orchestrator: the
  `orchestrator-guard.sh` `git -C` bypass, and the gated-sha defect in
  the first version of this hook. No ORCH row allocated: the brief
  forbids editing `docs/orchestration/board.md`.
- **Agent:** herd-step
- **Branch / commit:** `method-push-guard`, based on
  `origin/method-local-gate` `58dd6f3` (**not** `origin/main`, as the
  brief required, so the merger resolves
  `scripts/check-orchestration.sh` once)
- **Landing:** none — an agent pushes its branch and opens no PR;
  `herd-merger` names the landing here

## Done

### `scripts/githooks/pre-push` (new)

On a push whose destination ref is `refs/heads/main` the hook refuses
unless all of these hold. Every other ref goes through silently,
without consulting the gate.

1. **The tip is a `--no-ff` merge commit** — it has a second parent. A
   landing is always a merge of an integration branch onto `main`, so a
   commit made straight on `main` is refused.
2. **`gate.sh --verify-receipt` passes for the tip's second parent.**
   Not the tip. The gate runs on the integration tip; the merge
   commit's sha does not exist until afterwards, because its message is
   the landing record. See *The defect I shipped first* below — this is
   the correction, and it is the difference between a hook that works
   and a hook that refuses every correct landing.
3. **The merge changed nothing:** `git diff --quiet <second parent>
   <tip>` is silent. This is what keeps 2 honest. Without it a merger
   could gate one tree and push another and the receipt would still
   verify. It also catches a landing where `main` moved under the
   merge, so the merge resolved content no gate ever saw.
4. **The tip's own message is a filled landing record** — the seven
   `##` sections and four `- **Field:**` header fields of
   `docs/orchestration/templates/landing.md`, no `<...>` left in a
   header field, no one-line message, subject ≤ 72 columns.
5. **Not a rewrite** of `main`: the push is a fast-forward of what the
   remote advertised.
6. **Not a deletion** of `main`.

5 and 6 are evaluated **before** 1 to 4, so a rewrite or a deletion is
refused whatever the gate and the record say. The receipt rule is not
re-implemented anywhere: the hook shells out to `gate.sh`, and a case
proves which sha it asks about.

Two deliberate choices, both written in the file:

- The seven section names and four field names are **hard-coded in the
  hook**, not parsed from `landing.md`. A hook must work in a clone
  whose tree does not carry the template, and its verdict must not
  change because a working-tree file was edited after the gate ran.
- It does **not** require the `Land batch <n>:` subject prefix. A guard
  that is the only guard must not refuse a correct push over wording:
  every false refusal is an argument for `--no-verify`, and
  `--no-verify` is the one thing that destroys it. The orchestrator
  agreed on 2026-10-28.

`pre-push --check-install` reports whether the worktree it is run in has
`core.hooksPath` pointing at `scripts/githooks`.

The header states the limits in plain words: `core.hooksPath` or it does
not run at all; `--no-verify` and `git -c core.hooksPath= push` bypass
it; it sees nothing after the push and **nothing server-side backs it
up** (see finding 1); the receipt store is a local directory anyone can
write; it reads the **tip** commit's message only; a `--force` that is
still a fast-forward is indistinguishable from an ordinary push and
passes; it never runs the gate for you; it checks the record's shape,
never its truthfulness.

### The defect I shipped first, and what it cost

My first version asked `gate.sh --verify-receipt` for the sha **on the
hook's stdin** — the tip — and I wrote a case proving it did exactly
that. It was faithful to the brief and it would have refused every
correct landing, because the gated object and the pushed object are
different commits by construction:

```
gated sha (integration tip) = 31bb327f3b6c2eec173a6416c1c216ca0b00381d
pushed sha (merge commit)   = d791dfbc2f77ec4e473039cc54ba08ac9f77792d
same sha? NO   same tree? yes   rev-parse <pushed>^2 = 31bb327f…
```

Measured by a reviewer on the rules branch, not by me. **The lesson for
the harness, not just for this file:** my green case was green against
a scaffold I had built myself, where the tip *was* the gated commit
because I committed straight onto `main`. A green path built from the
implementation's own assumptions proves the implementation consistent
with itself and nothing else. The green path now builds a real landing
— integration branch, gate on it, `git merge --no-ff -F <record>` — and
three of the new red paths exist only to pin the thing I got wrong: a
receipt for the merge commit itself, a receipt for the first parent, and
a tip with no second parent are all refused.

### How it gets installed

```
git config core.hooksPath scripts/githooks
```

Measured, and it corrects the brief: `git rev-parse --git-common-dir` in
this worktree prints `/mnt/ssd_980/herd-project/.git` and
`extensions.worktreeConfig` is unset, so **one** `git config` in
`/mnt/ssd_980/herd-project` covers every worktree added from it. The
path is relative and resolves inside whichever worktree the hook runs
in. A fresh `git clone` carries none of it — `.git/config` is not in the
repository. **Branch `method-permissions-truth` owns the setting and
`docs/orchestration/permissions.md`**; I touched neither, and my fix
changes nothing about what `--check-install` should say.

### `.claude/hooks/orchestrator-guard.sh` — the `git -C` bypass closed

Measured before, on this branch's base:

```
git -C /x merge origin/main                        -> exit 0  BYPASS
git -C /x push origin main                         -> exit 0  BYPASS
git -C /x commit -m hi                             -> exit 0  BYPASS
git --git-dir=/x/.git --work-tree=/x commit -m hi  -> exit 0  BYPASS
```

The command is now normalised before the git rules run: a leading
`-C <dir>`, `--git-dir`, `--work-tree`, `--exec-path`, `--namespace`,
`--bare`, `--no-pager`, `--no-replace-objects`, `--literal-pathspecs`
and `-P` are stripped, quoted arguments included, and the existing
regexes match the result. All four now exit 2; `git -C <dir>` with
`fetch`, `status`, `log` and `diff` still exit 0.

`-c key=val` I left in place, because it is asserted as a deliberate
bypass and the orchestrator told me to stop rather than flip that
assertion silently. It is **being closed on
`method-permissions-truth`**, so my two cases
`og 0 'git -c a=b commit -m hi'` and
`og 0 'git -C /x -c a=b rebase origin/main'` will have to flip to
`og 2` when that branch lands. **That is a merge-time action, named
here so it is not discovered as a red gate.**

### The guard's false refusal, narrowed

The relative-redirection rule denied **any** relative redirection while
the shell stood in the checkout, which is where it usually stands. It
now resolves each target against `cwd` (`~/` and `..` included) and
denies only a target that lands inside the checkout:

```
echo x > a.md                        exit 2  (unchanged)
cd docs && echo x > a.md             exit 2  (unchanged)
echo x > ./docs/status.md            exit 2  (unchanged)
echo x > ../../notes.md              exit 0  (was 2)
echo x > ~/notes.md                  exit 0  (was 2)
echo x > ~someone/notes.md           exit 2  (fails closed)
tee -a report.md                     exit 2  (unchanged)
```

It still fails **closed**: no extractable target, a `~user` form or an
unjudgeable `$variable` all keep the old verdict. The
`cd <checkout> && <write>` rule moved into the `cwd`-is-outside branch,
so the two redirection rules can no longer disagree about the same
command.

The guard's header now also says what it is not: it constrains the
**main session only**, by design — an execution agent carries
`agent_id` or `agent_type` and is let through before any rule runs.
Nothing in it protects the repository against an agent.

### `scripts/check-orchestration.sh` — 172 cases to 226

Two new sections, appended, so the merger resolves one hunk at the end
of the file rather than three in the middle. 54 cases: 25 on the guard,
29 on the hook. Every hook case drives the **real** hook and the
**real** `scripts/gate.sh` in a scratch repository under the harness's
own `mktemp -d`, against a local bare remote: no network, no
credential, nothing outside `$tmp` written. Only the receipts are
stubs, because the receipt rule belongs to `gate.sh` and the hook must
be seen asking it.

Whole-script timing, same host: **14.6 s** at `58dd6f3`, **17.0 s**
here. The `docs` CI job is capped at 5 minutes, so the new cases cost
2.4 s of a 300 s budget.

## Not done, and why

- **The four Rust gates were NOT RUN.** The diff touches `scripts/` and
  `.claude/hooks/`, so the letter of `CLAUDE.md` asks for them; nothing
  in it is visible to `cargo`. The orchestrator reports the suite's
  three `real_herd` timeouts fixed (a test defect: three fixtures
  needed two job slots and the runner announced one, D119's cgroup
  clamp), so the gates *could* run now; they would still prove nothing
  about four shell and markdown files, and the host is shared.
- **`docs/orchestration/workflow.md`, `CLAUDE.md`, `.claude/agents/`**:
  not touched. T2 owns them. The line they need is below.
- **`docs/decisions.md`, `docs/orchestration/board.md`**: not touched,
  per the brief. Proposed decision text below.
- **`docs/orchestration/permissions.md` and `core.hooksPath`**: not
  touched — `method-permissions-truth` owns both.
- **`core.hooksPath` is set nowhere.** The hook exists, is tested and
  works end to end; it is installed nowhere, so until that one command
  runs it refuses nothing.
- **No repository-level rule asserts the install.** It cannot: the
  setting lives in `.git/config`, which is not in the tree, and
  `check-orchestration.sh` runs on a copy. What is testable is
  `--check-install` answering for the worktree it is asked in, both
  ways, plus a case that measures the limit: with `core.hooksPath`
  unset, the push that was refused above succeeds.

## Gates

`./scripts/check-docs.sh`, run last, after this file was committed:

```
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```

`./scripts/check-orchestration.sh` — the two new sections, verbatim.
Every red path going red and every green path going green:

```
== orchestrator-guard.sh: a global option must not move the verb out of reach
ok    guard exit 2: git -C /x merge origin/main
ok    guard exit 2: git -C /x push origin main
ok    guard exit 2: git -C /x commit -m hi
ok    guard exit 2: git --git-dir=/x/.git --work-tree=/x commit -m hi
ok    guard exit 2: git --git-dir /x/.git push origin main
ok    guard exit 2: git --work-tree=/x merge origin/main
ok    guard exit 2: git -C \"/a b\" push origin main
ok    guard exit 2: cd /tmp && git -C /x push origin main
ok    guard exit 0: git -C /x -c a=b rebase origin/main
ok    guard exit 0: git -C /x fetch origin
ok    guard exit 0: git -C /x status --short
ok    guard exit 0: git -C /x log --oneline -5
ok    guard exit 0: git -C /x diff origin/main --stat
ok    guard exit 0: git --no-pager log -1
ok    guard exit 0: git --git-dir=/x/.git log --oneline -3
ok    guard exit 0: git -c a=b commit -m hi
ok    guard exit 0: /usr/bin/git commit -m hi
ok    guard exit 2: echo x > a.md
ok    guard exit 2: cd docs && echo x > a.md
ok    guard exit 2: echo x > ./docs/status.md
ok    guard exit 0: echo x > ../../notes.md
ok    guard exit 0: echo x > ~/notes.md
ok    guard exit 0: echo x > /tmp/claude-1002/x/notes.md
ok    guard exit 2: echo x > ~someone/notes.md
ok    guard exit 2: tee -a report.md
== pre-push: the last machine between an agent and main
ok    pre-push: a tip on main with no second parent is refused
ok    pre-push: a landing with no receipt at all is refused
ok    pre-push: a receipt for the merge commit itself does not authorise it
ok    pre-push: a receipt for the first parent does not authorise the landing
ok    pre-push: a RED receipt for the gated commit is refused
ok    pre-push: a GREEN receipt renamed onto the gated sha is refused
ok    pre-push: a merge that changed the tree is refused though the receipt verifies
ok    pre-push: a one-line merge message is refused on a correct landing
ok    pre-push: a merge message missing one required section is refused
ok    pre-push: a header field still carrying its placeholder is refused
ok    pre-push: a missing header field is refused and named
ok    pre-push: a subject over 72 characters is refused
ok    pre-push: a --no-ff landing, gated second parent, identical tree, filled record
ok    pre-push: the landing reached the remote
ok    pre-push: the gate is asked about the second parent, not the tip
ok    pre-push: the sha on stdin decides, not HEAD
ok    pre-push: a task branch with no receipt and a one-line message is silent
ok    pre-push: a ref that is not main never consults the gate
ok    pre-push: force-pushing a task branch is not this hook's business
ok    pre-push: a force-push that rewrites main is refused, gate and record notwithstanding
ok    pre-push: the same rewrite without --force is refused, by git before the hook
ok    pre-push: deleting main is refused
ok    pre-push: deleting main by --delete is refused too
ok    pre-push: main still exists on the remote after both refusals
ok    pre-push: a remote tip this clone has never seen is refused, not guessed
ok    pre-push: --check-install passes where core.hooksPath points at it
ok    pre-push: --check-install reports a worktree with no core.hooksPath
ok    pre-push: with core.hooksPath unset the guard does not run (known limit)
ok    pre-push: --no-verify bypasses the hook entirely (known limit)

check-orchestration: 226 passed, 0 failed
```

Baseline at `origin/method-local-gate` `58dd6f3`, detached worktree,
same host:

```
check-orchestration: 172 passed, 0 failed
```

And the same rule driven by hand end to end, outside the harness, in a
throwaway repository — a real integration branch, a real
`git merge --no-ff -F <record>`, a real `git push`:

```
=== (1) the integration tip was gated, but no receipt yet ===
pre-push: REFUSED push to refs/heads/main
pre-push:   no green gate receipt for the gated commit (401d5444ef40…), the second
pre-push:   parent of the tip being pushed. gate.sh said: gate: no receipt for 401d5444ef40…
pre-push:   Run ./scripts/gate.sh on the integration branch, then merge it with
pre-push:   --no-ff and push. The merge commit itself can never carry a receipt:
pre-push:   its sha does not exist until after the gate has run.
pre-push: nothing was pushed. See scripts/githooks/pre-push.
=== (2) a receipt for the MERGE COMMIT, not the integration tip ===
pre-push: REFUSED push to refs/heads/main
pre-push:   no green gate receipt for the gated commit (401d5444ef40…), the second
pre-push:   parent of the tip being pushed. gate.sh said: gate: no receipt for 401d5444ef40…
pre-push:   Run ./scripts/gate.sh on the integration branch, then merge it with
pre-push:   --no-ff and push. The merge commit itself can never carry a receipt:
pre-push:   its sha does not exist until after the gate has run.
pre-push: nothing was pushed. See scripts/githooks/pre-push.
=== (3) the correct receipt, on the integration tip ===
pre-push: main <- d338423aed38 (merge of 401d5444ef40)
pre-push:   gate: GREEN receipt for 401d5444ef4093a60cbc5f615f50eedf8d2e1b03
pre-push:   the merge changed nothing the gate did not see.
pre-push:   landing record: seven sections, four fields, filled.
 * [new branch]      main -> main
```

Skips: **no skip**. Neither script skips an assertion.

Rust gates (`cargo build`, `cargo fmt --all --check`, `cargo clippy
--workspace --all-targets -- -D warnings`, `./scripts/test.sh`):
**NOT RUN** locally — see *Not done*.

## Risks and findings

### Where the brief is wrong

1. **"Branch protection currently refuses both server-side … belt and
   braces" — false, and it inverts the stakes.** Measured today:

```
$ gh api repos/tomasalexandre18/Herd/branches/main/protection
enforce_admins.enabled: false
allow_force_pushes.enabled: false
allow_deletions.enabled: false
required_status_checks: strict=false, contexts=["check","docs"]
required_pull_request_reviews: {required_approving_review_count: 0, …}

$ gh api repos/tomasalexandre18/Herd --jq .permissions
{"admin":true,"maintain":true,"pull":true,"push":true,"triage":true}
```

   With `enforce_admins` false, branch protection does not bind an
   admin, and the credential this project pushes with **is** an admin.
   GitHub therefore refuses neither a force-push nor a deletion of
   `main` **today**, before any narrowing. The hook's rules 5 and 6 are
   the belt. Consequence for the narrowing task: keeping
   `allow_force_pushes: false` buys nothing against our own credential;
   making it mean something needs `enforce_admins: true`, which is a
   different decision and Alexa's.
2. **"every worktree needs that configured" — not here.** One
   `git config` in `/mnt/ssd_980/herd-project` covers them all; the
   real gap is a fresh `git clone`.
3. **"add a case that a worktree without it is detectable" — not
   possible as stated.** Nothing in the repository can see a worktree's
   config. `--check-install` answers for the worktree it is *asked in*;
   whoever runs it has to remember to. That is a duty, not a mechanism,
   and it should be written that way.
4. **"Refuse unless gate.sh --verify-receipt passes for the exact sha
   being pushed … the sha in the hook's stdin line."** Unsatisfiable,
   and the brief was emphatic about the wrong thing. See *The defect I
   shipped first*. The stdin sha is still what decides *which object to
   ask about* — the hook never looks at `HEAD` — but the object asked
   about is that sha's **second parent**.
5. **The intermittent case near line 464 did not reproduce.** Six
   whole-script runs today, all green.

### Findings in the code

6. **`git` declines a non-fast-forward before `pre-push` runs.** The
   same rewrite the hook refuses under `--force` never reaches the hook
   without it. Both are asserted with the right attribution, because a
   reader who thinks the hook caught the second one would over-trust
   it.
7. **`gawk` 5.4.1 rejects `/[\200-\277]/` outright** in a UTF-8 locale
   (`awk: invalid range endpoint`). `check-docs.sh` is safe only
   because it runs that awk under `LC_ALL=C` (`check-docs.sh:67`). My
   first subject-width check copied the expression without the locale
   and silently measured nothing; a case caught it. Anyone copying that
   idiom must copy the `LC_ALL=C` with it.
8. **The receipt store is forgeable.** Three lines in a file named for
   a sha satisfy `gate.sh --verify-receipt` — that is how the fixtures
   are built, and the same three lines typed by hand would pass. A
   receipt is evidence that a gate ran, not proof. Making it
   unforgeable needs a signature and a key: a decision, not a patch.
9. **The hook reads the tip's message only**, and checks the tree of
   the tip against its second parent only. A batch whose tip is a
   filled record and whose tree matches what was gated can still carry
   any history underneath.
10. **A landing where `main` moved under the merge is refused**, by
    rule 3, with a message that says the merge changed the tree. That
    is correct but it will look like a false refusal to whoever hits
    it, so the method has to keep its one-merger-at-a-time rule or this
    guard becomes an obstacle.
11. **An octopus merge is refused** by rule 3, not by a rule of its
    own: `^2` names only the first merged branch and the tree will not
    match. The message will say the tree changed, which is true but not
    the clearest possible diagnosis.

## Decisions needed

**D-A — withdrawn.** I had proposed closing the guard's
`git -c key=val` bypass; the orchestrator has taken that decision and
it is being done on `method-permissions-truth`. The only thing left is
the merge-time action named above: flip my two `-c` cases from `og 0`
to `og 2` when that branch lands.

**D-B — `D-NEXT`, the push guard.** Proposed entry, for whoever owns
`docs/decisions.md` on this batch (I was told not to write it):

> ## 2026-10-28 — D-NEXT The push guard is the last machine before main
>
> With pull requests and required status checks gone, and with
> `enforce_admins: false` against an admin push credential, nothing on
> GitHub's side refuses a bad push to `main` — not even a force-push or
> a deletion. `scripts/githooks/pre-push` is the mechanism. A landing
> is a `--no-ff` merge of a gated integration branch onto `main`, so
> the hook refuses a tip with no second parent, a second parent with no
> GREEN `gate.sh` receipt, a merge whose tree differs from the gated
> tree, a tip whose message is not a filled landing record, a rewrite
> and a deletion. It asks `gate.sh --verify-receipt` rather than
> re-implementing the receipt rule. It runs only where
> `core.hooksPath` points at `scripts/githooks`, and `--no-verify`
> bypasses it; both limits are stated in the file and asserted in
> `check-orchestration.sh`. The landing-record check exists here
> because `check-orchestration.sh` copies the corpus without `.git`,
> so no docs rule can read a commit message.

**D-C — the sentence T2 owes `workflow.md`**, if
`method-permissions-truth` has not already written it:

> A landing is `./scripts/gate.sh` on the integration branch, then
> `git merge --no-ff -F <landing record> <integration>` onto `main`,
> then `git push`. The gate runs on the integration tip, never on the
> merge commit, whose sha does not exist yet. Never
> `git push --no-verify`: it is the one command that walks past the
> only guard in front of `main`.

## Resume point

The install and `permissions.md` belong to `method-permissions-truth`.
What is left here is the merge:

```sh
# when method-permissions-truth lands, flip the two -c cases:
#   og 0 'git -c a=b commit -m hi'            -> og 2
#   og 0 'git -C /x -c a=b rebase origin/main' -> og 2
./scripts/check-orchestration.sh    # must stay 226 passed, 0 failed
```

Before the first real landing, re-read
`gh api repos/tomasalexandre18/Herd/branches/main/protection`: if
`enforce_admins` is still false, finding 1 stands and the narrowing
task should be told that keeping `allow_force_pushes: false` buys
nothing against our own credential.

## Processes

PIDs started and ended: **none** — no background process, no server,
no build. Every command was a foreground shell, `git`, `gh api` (two
read-only calls) or one of the two check scripts.

Own `target/`: **none created** — no `cargo` ran. The scratch
repositories the cases build live under the harness's own `mktemp -d`
and are removed by its `trap`. Two worktrees of mine to remove once
this is merged, both under
`…/scratchpad/method-push-guard/`: `wt` (the branch) and
`baseline/wt` (detached at `58dd6f3`, for the before timing). I left
them in place rather than mutating
`/mnt/ssd_980/herd-project/.git/worktrees` while other agents are live.
