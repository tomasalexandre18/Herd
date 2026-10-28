# method-push-guard — handoff 2026-10-28

- **Item:** T5 of the local-merge method (plan of 2026-10-28,
  herd-planner), plus the `orchestrator-guard.sh` `git -C` bypass sent
  mid-task by the orchestrator. No ORCH row allocated: the brief forbids
  editing `docs/orchestration/board.md`.
- **Agent:** herd-step
- **Branch / commit:** `method-push-guard` / see *Gates*, based on
  `origin/method-local-gate` `58dd6f3` (**not** `origin/main`, as the
  brief required, so the merger resolves
  `scripts/check-orchestration.sh` once)
- **Landing:** none — an agent pushes its branch and opens no PR;
  `herd-merger` names the landing here

## Done

### `scripts/githooks/pre-push` (new, 228 lines)

The hook refuses a push whose destination ref is `refs/heads/main`
unless all four hold, and lets every other ref through silently:

1. `./scripts/gate.sh --verify-receipt <sha>` passes for the exact sha
   on the hook's stdin. The receipt rule is **not** re-implemented: the
   hook shells out to `gate.sh`, and a case proves it asks about the
   stdin sha and not about `HEAD`.
2. the tip commit's own message is a filled landing record — the seven
   `##` sections and the four `- **Field:**` header fields of
   `docs/orchestration/templates/landing.md` (read from branch
   `method-landing-record`), with no header field still carrying an
   `<...>` placeholder, no one-line message, and a subject ≤ 72
   columns.
3. the push is a fast-forward of what the remote advertised.
4. it is not a deletion of `main`.

Checks 4 and 3 run **before** 1 and 2, so a deletion or a rewrite is
refused whatever the gate and the record say.

Two deliberate design choices, both written in the file:

- The seven section names and four field names are **hard-coded in the
  hook**, not parsed out of `landing.md`. A hook must work in a clone
  whose tree does not carry the template, and its verdict must not
  change because a working-tree file was edited after the gate ran.
- The hook does **not** require the `Land batch <n>:` subject prefix. A
  guard that is the only guard must not refuse a correct push over a
  wording quibble: every false refusal is an argument for
  `--no-verify`, and `--no-verify` is the one thing that destroys it.

`pre-push --check-install` reports whether the worktree it is run in
has `core.hooksPath` pointing at `scripts/githooks`.

The header states the limits in plain words, as the brief required:
`core.hooksPath` or it does not run at all; `--no-verify` and
`git -c core.hooksPath= push` bypass it; it sees nothing after the
push; the receipt store is a local directory anyone can write; it reads
the **tip** commit only; a `--force` that is still a fast-forward is
indistinguishable from an ordinary push and passes; it never runs the
gate for you; it checks the record's shape, never its truthfulness.

### How it gets installed

One command, in the checkout or any worktree of it:

```
git config core.hooksPath scripts/githooks
```

Measured, and it changes the brief: `git rev-parse --git-common-dir` in
this worktree prints `/mnt/ssd_980/herd-project/.git` and
`extensions.worktreeConfig` is unset, so **one** `git config` in
`/mnt/ssd_980/herd-project` covers every worktree added from it. The
path is relative and resolves inside whichever worktree the hook runs
in. A fresh `git clone` carries none of it — `.git/config` is not in
the repository — so the line belongs in `workflow.md`'s
worktree-creation step, which **T2 owns**. I did not run that command:
it writes the config shared with the orchestrator's checkout, which is
outside my paths.

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

`-c key=val` is **deliberately not** in that list. The orchestrator's
message said to stop rather than silently close a bypass that is
asserted as deliberate, and `git -c a=b commit` is asserted at the
"known bypasses" section of `check-orchestration.sh`. So both
documented bypasses keep their verdict, byte-identical assertions, and
the remaining hole is now measured by a case of its own rather than
left to be discovered: `git -C /x -c a=b rebase origin/main` exits 0.
See *Decisions needed*.

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

### `scripts/check-orchestration.sh` — 172 cases to 223

Two new sections, appended, so the merger resolves one hunk at the end
of the file rather than three in the middle. 51 cases: 25 on the guard,
26 on the hook. Every hook case drives the **real** hook and the
**real** `scripts/gate.sh` in a scratch repository under `$tmp` with a
local bare remote: no network, no credential, nothing outside `$tmp`
written. Only the receipts are stubs, because the receipt rule belongs
to `gate.sh` and the hook must be seen asking it.

Whole-script timing, same host, same minute: **14.6 s** at `58dd6f3`,
**16.5 s** here. The `docs` CI job is capped at 5 minutes, so the new
cases cost 2 s of a 300 s budget. Three consecutive runs gave
`223 passed, 0 failed`.

## Not done, and why

- **The four Rust gates were NOT RUN.** The diff touches `scripts/` and
  `.claude/hooks/`, so the letter of `CLAUDE.md` asks for them; nothing
  in it is visible to `cargo`, and the brief said to report them not
  run with the reason rather than take a build slot from the agent
  hunting the suite defect. The suite is also known RED on this host on
  three `real_herd` 30 s timeouts, which is not this branch's doing.
- **`docs/orchestration/workflow.md`, `CLAUDE.md`, `.claude/agents/`**:
  not touched, per the brief. T2 owns them. The one line they need is
  under *Decisions needed*.
- **`docs/decisions.md` and `docs/orchestration/board.md`**: not
  touched, per the brief. Proposed decision text below.
- **`docs/orchestration/permissions.md`**: not touched — it documents
  the known bypasses and is now slightly conservative, since `git -C`
  is closed. It is not in my paths. Owner needed.
- **`core.hooksPath` is not set anywhere.** The hook exists and is
  tested; it is installed nowhere. Until somebody runs the one command,
  it refuses nothing. That is the resume point.
- **No `check-docs.sh` rule asserts the install.** It cannot: the
  setting lives in `.git/config`, which is not in the repository, and
  `check-orchestration.sh` runs on a scratch copy. What is testable is
  `--check-install` answering for the worktree it is asked in, and the
  instruction existing in `workflow.md` — a T2 assertion.

## Gates

`./scripts/check-docs.sh`, run last, after the handoff was written:

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
ok    pre-push: main with no receipt at all is refused
ok    pre-push: a receipt for another sha does not authorise this one
ok    pre-push: a RED receipt for this sha is refused
ok    pre-push: a GREEN receipt renamed onto this sha is refused
ok    pre-push: a one-line tip message is refused even with a green receipt
ok    pre-push: a tip message missing one required section is refused
ok    pre-push: a header field still carrying its placeholder is refused
ok    pre-push: a missing header field is refused and named
ok    pre-push: a subject over 72 characters is refused
ok    pre-push: a green receipt for this sha plus a filled record lands
ok    pre-push: the green push reached the remote
ok    pre-push: the hook asks the gate about the sha on its stdin
ok    pre-push: pushing a commit that is not HEAD is judged on that commit
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

check-orchestration: 223 passed, 0 failed
```

Baseline for comparison, `origin/method-local-gate` `58dd6f3` in a
detached worktree, same host:

```
check-orchestration: 172 passed, 0 failed
```

Skips: **no skip**. Neither script skips an assertion, and the Rust
suite was not run.

Rust gates (`cargo build`, `cargo fmt --all --check`, `cargo clippy
--workspace --all-targets -- -D warnings`, `./scripts/test.sh`):
**NOT RUN** locally — see *Not done*. Nothing in the diff is visible to
`cargo`.

## Risks and findings

### Where the brief is wrong

1. **"Branch protection currently refuses both server-side and will
   keep doing so after the narrowing, but belt and braces."** False,
   and it inverts the stakes. Measured today:
   `gh api repos/tomasalexandre18/Herd/branches/main/protection` gives
   `enforce_admins.enabled: false`, `allow_force_pushes.enabled:
   false`, `allow_deletions.enabled: false`; and
   `gh api repos/tomasalexandre18/Herd --jq .permissions` gives
   `{"admin":true,...}` for the credential this project pushes with.
   With `enforce_admins` false, branch protection does not bind an
   admin, so **GitHub refuses neither a force-push nor a deletion of
   `main` today**, before any narrowing. The hook's checks 3 and 4 are
   not belt and braces: they are the belt. The orchestrator's mid-task
   message says the same thing, and this is the measurement behind it.
2. **"every worktree needs that configured"** — not here. `git
   worktree` worktrees share `$GIT_COMMON_DIR/config` and
   `extensions.worktreeConfig` is unset, so one `git config` in
   `/mnt/ssd_980/herd-project` covers them all. The real gap is a fresh
   `git clone`, which carries no config at all.
3. **"add a `check-orchestration.sh` case that a worktree without it is
   detectable"** — a worktree without `core.hooksPath` cannot be
   detected by anything in the repository: the setting is not in the
   tree, and `check-orchestration.sh` works on a copy. What I could
   build is `--check-install`, which answers for the worktree it is
   *asked in*, with a case each way. Whoever runs it has to remember
   to; that is a duty, not a mechanism, and it should be said that way
   wherever it is written down.
4. **The intermittent case near line 464 did not reproduce.** Three
   consecutive whole-script runs, `223 passed, 0 failed` each time,
   including runs launched from a shell call that also wrote files.
   Not fixed, not reproduced — treat ORCH-111 as still open and
   unmeasured rather than as cleared by me.

Everything else in the brief held. `gate.sh --verify-receipt` behaves
exactly as described, including refusing a GREEN receipt renamed onto
another sha; `58dd6f3` is the right base and carried 172 cases.

### Findings in the code

5. **`git` declines a non-fast-forward before `pre-push` runs.** The
   same rewrite that the hook refuses under `--force` never reaches the
   hook without it — git rejects it during the push negotiation. Both
   paths are asserted, with the right attribution, because a reader who
   believes the hook caught the second one would over-trust it.
6. **`gawk` 5.4.1 rejects `/[\200-\277]/` outright** in a UTF-8 locale:
   `awk: invalid range endpoint`. `check-docs.sh` is safe only because
   it runs that awk under `LC_ALL=C` (`check-docs.sh:67`). My first
   version of the subject-width check copied the expression without the
   locale and silently measured nothing; the case caught it. Anyone
   copying that idiom must copy the `LC_ALL=C` with it.
7. **The hook fails closed on a missing `gate.sh`** — a push to `main`
   from a tree with no `scripts/gate.sh` is refused, not allowed.
8. **`--no-verify` and an unset `core.hooksPath` are asserted as
   bypasses**, with cases that show the push succeeding. They are the
   hook's limits, and the harness now records them as measurements
   rather than as prose nobody re-checks.
9. **The receipt store is forgeable by anyone who can write it.** Three
   lines in a file named for a sha satisfy `gate.sh --verify-receipt`;
   that is how the cases build their fixtures, and the same three lines
   typed by hand would pass. A receipt is evidence that a gate ran, not
   proof. Making it unforgeable needs a signature and a key, which is a
   decision, not a patch.
10. **The hook reads the tip commit only.** A batch whose tip is a
    filled landing record can carry anything underneath it.

## Decisions needed

**D-A — close the `git -c key=val` bypass, or write down that it
stays?** *Question:* `git -c a=b commit` and
`git -C /x -c a=b rebase` pass the orchestrator guard, and `-c` is the
one option I did not strip, because it is an asserted deliberate
bypass. Do we close it — one line in the strip list, one assertion
flipped from 0 to 2 in `check-orchestration.sh`, one sentence in
`docs/orchestration/permissions.md` — or keep it? *Recommendation:*
close it. The reason it was accepted (a regex cannot win against a
determined caller) is still true, but `git -c` is not a determined
caller's tool either, and after the narrowing the guard is one of two
mechanisms left rather than a convenience in front of a required
status check. *Fallback:* keep it; the case I added measures it, so it
can no longer be rediscovered as a surprise. *Not mine to take:* the
orchestrator told me to stop rather than edit those assertions, and
`permissions.md` is not in my paths.

**D-B — `D-NEXT`, the push guard.** Proposed entry, for whoever owns
`docs/decisions.md` on this batch (I was told not to write it):

> ## 2026-10-28 — D-NEXT The push guard is the last machine before main
>
> With pull requests and required status checks gone, and with
> `enforce_admins: false` against an admin push credential, nothing on
> GitHub's side refuses a bad push to `main` — not even a force-push or
> a deletion. `scripts/githooks/pre-push` is the mechanism: it refuses
> a push to `refs/heads/main` without a GREEN `gate.sh` receipt for the
> exact sha, without a filled landing record as the tip commit's
> message, that rewrites history, or that deletes the branch. It runs
> only where `core.hooksPath` points at `scripts/githooks`, and
> `--no-verify` bypasses it; both limits are stated in the file and
> asserted in `check-orchestration.sh`. The landing record check exists
> here because `check-orchestration.sh` copies the corpus without
> `.git`, so no docs rule can read a commit message.

**D-C — the sentence T2 owes `workflow.md`.** One line in the
worktree-creation step, and it is T2's file, not mine:

> After `git worktree add`, run `git config core.hooksPath
> scripts/githooks` once in `/mnt/ssd_980/herd-project` (worktrees
> share its config); `./scripts/githooks/pre-push --check-install`
> says whether it took. Never `git push --no-verify`: it is the one
> command that walks past the only guard in front of `main`.

## Resume point

```sh
cd /mnt/ssd_980/herd-project && git config core.hooksPath scripts/githooks
cd <any worktree> && ./scripts/githooks/pre-push --check-install
```

Then T2 writes D-C's sentence into `workflow.md`, and the merger of
this batch is the first landing the hook will judge. Before that
landing, re-read
`gh api repos/tomasalexandre18/Herd/branches/main/protection`: if
`enforce_admins` is still false, finding 1 stands and the narrowing
task should be told that keeping `allow_force_pushes: false` buys
nothing against our own credential.

## Processes

PIDs started and ended: **none** — no background process, no server,
no build. Every command was a foreground shell, `git`, `gh api` (two
read-only calls) or one of the two check scripts.

Own `target/`: **none created** — no `cargo` ran, so no
`CARGO_TARGET_DIR` to delete. The scratch repositories the cases build
live under the harness's own `mktemp -d` and are removed by its `trap`.
Two worktrees to remove once this is merged:
`.../scratchpad/method-push-guard/wt` (the branch) and
`.../scratchpad/method-push-guard/baseline/wt` (detached at `58dd6f3`,
for the before timing).
