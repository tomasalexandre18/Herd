# merge herd-own-claude-root — handoff 2026-10-26

- **Item:** ORCH-030 (with ORCH-066, ORCH-067, ORCH-068, ORCH-069 opened
  by this integration)
- **Agent:** herd-merger
- **Branch / commit:** `herd-own-claude-root` / merge commit `7ba3103`,
  first parent `768311e` (the branch), second parent `c1d5ce7`
  (`origin/main`), based on `origin/main` `c1d5ce7`
- **PR:** #72

## Done

**The branch as it stood.** `3da48f5` was the merge base: five files,
`README.md`, `docs/decisions.md`, `docs/orchestration/state-root.md`,
`ops/agents/env.sh`, `ops/agents/session.sh`. No Rust, no `Cargo.*`.

**`main` had moved, further than the brief said.** The brief read `main`
at `860c4f5` (PR #75). It was at `c1d5ce7` (PR #84): `860c4f5` is an
ancestor, and five more PRs — #76 to #84 — had landed, carrying the whole
orchestration corpus, the C0b pass and C5 parts 1 and 2. That is why
GitHub said `CONFLICTING`: 121 files changed on `main`'s side against
five on the branch's, and exactly **one** file touched by both.

**One conflict, in `docs/decisions.md`**, at the tail, where both sides
appended after D171:

| | What the side said |
|---|---|
| `HEAD` (branch) | 83 lines: `---`, then `## 2026-10-24 — D174 Herd's sessions keep their state in Herd's own root` |
| `origin/main` | 571 lines: D173, D172 (from #71, #70) and D175, D176, D177, D178 (from the C0b pass and the C5 stack) |

**Resolved by keeping both, `main`'s block first and D174 appended after
it.** Nothing renumbered. D174 is kept because it is still free: #70 and
#71 took D172 and D173, and the four allocations since took D175 to D178
and left D174 alone, every one of them saying so in its own "On the
number" paragraph. `gh pr list` confirms no other open PR claims it —
#85 carries three `D-NEXT` placeholders and #67 holds D170. D143, D154
and D170 stay gaps.

The entry sits after D178 although it is dated a day earlier, because
that is what this file does: `docs/orchestration/decisions-structure.md`
records D148 after D149 and D161 after D167 for the same reason, "a
branch that merged late appended at the end with the number it had taken
earlier", and D148 is the rule that a number is an identifier and not a
position. `check-docs.sh` enforces uniqueness and the header shape, not
monotonicity. **This differs from the brief**, which asked for date
order; the brief's own rule, `herd-merger`'s contract and the file's two
precedents all say append, so appending is what was done, and it is said
here rather than passed over.

**One paragraph of the new entry was restated**, D174's own "On the
number", which said "D172 and D173 are held by unmerged branches (PR
#70, PR #71)" — true when written, false at integration. It now says the
number was taken on 2026-10-24, that #70 and #71 have since merged, that
D175 to D178 left D174 free, and where the entry appends and why. No
other line of any entry, on either side, was edited.

### Nothing was lost, proved four ways

1. **Headings.** `main` 174, branch 169, base 168, resolved **175** =
   174 + 1. Every `## ` line of `main` is in the result; every `## ` line
   of the branch is too; the result has no heading that was on neither
   side. Same set of D numbers as the union of both sides, no duplicate
   (`sort -n | uniq -d` empty), gaps 143, 154, 170.
2. **Each side's pre-merge file is reconstructible from the conflict.**
   The 8 173 lines before the marker plus the `HEAD` block reproduce the
   branch's `docs/decisions.md` byte for byte (`cmp` silent); the same
   prefix plus the `origin/main` block reproduce `main`'s, 493 609 bytes,
   byte for byte.
3. **Deleting this PR's added block reproduces `main`.** `head -n -90`
   of the resolved file `cmp`s identical to `origin/main`'s file, 493 609
   bytes. Deleting `main`'s 572 added lines instead reproduces the
   branch's file with the restated paragraph as the only difference.
4. **The diff is additive.** `git diff --numstat origin/main HEAD --
   docs/decisions.md` is `90 0`: ninety insertions, **zero** deletions.

**The merged tree differs from `origin/main` by the branch's five files
and nothing else**, and the merge changed none of the branch's own files:
`git diff 768311e 7ba3103 -- ops/ docs/orchestration/state-root.md
README.md` is empty.

**Board.** ORCH-030 (`blocked` → `done`), and four rows opened:
ORCH-066 (the credentials sync unit, blocked, the owner's), ORCH-067 (the
`CLAUDE.md` pointer and the gotcha that still reads as current, owed),
ORCH-068 (the live root's own launchers against the shipped ones,
blocked), ORCH-069 (the red assertion below). `Next` item 5 rewritten and
item 6 added. No row was removed and no open item lost its state.

## Not done, and why

- **`docs/status.md` untouched.** It is the state of the software: no
  crate, no protocol field, no record, no release and no running
  behaviour changes here, and it is already at exactly 90 of its 90
  lines. The orchestration state this integration moved belongs to the
  board, which is where it went.
- **`CLAUDE.md` still does not point at `state-root.md`.** The branch
  left that line out on purpose while #71 was open; #71 landed on
  2026-10-24, so the reason is gone, but adding it is a content change
  this integration did not make. It is ORCH-067, one line, prose gates
  only.
- **The task's own handoff does not exist.** There is no
  `history/<date>-herd-own-claude-root.md`: the branch is based on
  `3da48f5`, which predates `docs/orchestration/history/` entirely. The
  record of the work is the PR body, D174 and `state-root.md`; this file
  is the integration's handoff and does not pretend to be the task's.
  Nothing was written on the task author's behalf.
- **Nothing in the live state root was touched.** `/mnt/ssd_980/herd` was
  read — `ls`, and `grep` on `claude/env.sh` — and not written, moved,
  copied or deleted. `.credentials.json` was never opened.
- **No tag, no release, no issue, nothing on the consumer's side.**

## Gates

The resolved tree adds `ops/agents/*.sh`, outside `CLAUDE.md`,
`README.md`, `docs/`, `.claude/` and `.github/*.md`, so all four Rust
gates were owed and all four were run, plus `check-docs.sh` and the
`check-orchestration.sh` that CI's `docs` job runs.

```text
$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
check_docs_exit=0

$ ./scripts/check-orchestration.sh
check-orchestration: 87 passed, 0 failed
check_orchestration_exit=0

$ nix develop --command cargo fmt --all --check
fmt_exit=0

$ nix develop --command cargo build --workspace
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 23.63s
build_exit=0

$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 12.88s
clippy_exit=0

$ nix develop --command ./scripts/test.sh
thread 'the_derived_j_is_recorded_and_is_what_the_job_is_given' panicked at herd-hub/tests/slots.rs:744:9:
assertion `left == right` failed: the figure E-22 asks for, in the process's own words.
  left: "built with -j4"
 right: "built with -j7"
test result: FAILED. 7 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
Every assertion ran on this host: nothing was skipped.
test suite: FAILED (cargo test exited 101)
test_exit=101

$ nix develop --command ./scripts/test.sh --no-fail-fast   # to see all of it
766 passed across 48 binaries; 2 failed:
  herd-hub/tests/slots.rs:744  the_derived_j_is_recorded_and_is_what_the_job_is_given
      left: "built with -j3"   right: "built with -j7"
  herd-hub/tests/admission.rs:419  a_killed_hub_reads_the_reservations_back_off_the_records
      the inherited job's 4 GiB of declared memory must still be held:
      ... 2.8 GiB free of its 6.8 GiB memory admission, 5 GiB needed ...
      (the assertion wants the string "3 GiB free of its 7 GiB memory admission")
==============================================================
 12 assertion(s) COULD NOT RUN on this host and were skipped.
 A green run does NOT mean these were checked.
==============================================================
test suite: FAILED (cargo test exited 101)
test_nofailfast_exit=101
```

**The suite is RED on this host — two assertions — and it is red with no
branch content at all.** The branch adds no Rust and no `Cargo.*` line,
so it cannot be the cause, and that is not left as an argument. Both
tests were run on a detached worktree of `origin/main` `c1d5ce7`, with
not one line of this branch present:

```text
$ nix develop --command cargo test -p herd-hub --test slots \
    the_derived_j_is_recorded_and_is_what_the_job_is_given    # origin/main c1d5ce7
  left: "built with -j3"   right: "built with -j7"
exit_on_origin_main=101

$ # admission, alternating the two trees, memory measured before each run
round 1 origin/main c1d5ce7   MemFree 2 683 724 kB  MemAvailable 21 522 384 kB  FAILED
round 1 merged tree           MemFree 2 402 928 kB  MemAvailable 21 252 752 kB  FAILED
round 2 origin/main c1d5ce7   MemFree 2 454 544 kB  MemAvailable 21 304 260 kB  FAILED
round 2 merged tree           MemFree 2 716 472 kB  MemAvailable 21 569 180 kB  FAILED
```

Both are the same defect: an assertion on a figure the hub **derived
from the host's live memory**, compared to a constant written for another
machine.

- `slots.rs:744` wants E-22's `-j7`. It guards itself on `MemTotal >= 17
  GiB` and skips below it (D27) — but the `-j` is derived from what the
  runner can actually *lend* at that instant. This host has 31.8 GiB of
  `MemTotal` with ~10.6 GiB in use, the runner lent 9 924 MiB, and at
  1 792 MiB per build thread that is `-j3` or `-j4`. The guard passes on
  a quantity the assertion does not use, so a case that should have been
  skipped is asserted instead. `-j3` and `-j4` in runs minutes apart is
  the signature.
- `admission.rs:419` wants the sentence "3 GiB free of its 7 GiB memory
  admission, 5 GiB needed" and gets "2.8 GiB free of its 6.8 GiB memory
  admission, 5 GiB needed". Nothing about the reservation logic it exists
  to check is wrong in that line — the inherited job's 4 GiB *is* still
  held; it is the rounded total that does not match.

One honest caveat on the method: `admission.rs` passed once on
`origin/main` when run alone, before the four alternating runs above
found it red on both trees in both rounds. So it is red on `main` without
this branch, which is the claim being made, and that single earlier pass
is reported rather than dropped.

This is ORCH-069. Neither test is in ORCH-022's list of three, nothing
was edited to make either green, and no skip was turned into a pass.
A local suite on this workstation is not a verdict on a branch while
these two stand; CI's `check` job on a GitHub runner is, and that is what
the merge decision used.

Skips: the verbatim gate run reports none, because `cargo test` stops at
the first failing binary before any skipping assertion runs. The
`--no-fail-fast` run does reach them and prints exactly the **12** the
brief expected: 10 for cgroup v2 delegation being unavailable, 2 for the
ZFS `io.max`, those two being
`a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device`
and `a_declared_ceiling_changes_what_the_inert_weight_line_says`. Each
is printed as a skip and none is reported as a pass (D27).

## Risks and findings

1. **ORCH-069, the two red assertions above.** Reported, not worked
   around. No test, no assertion and no skip guard was edited to make
   anything green.
2. **This session still runs inside chatOnS's systemd scope.** The skip
   lines name it:
   `/sys/fs/cgroup/chatons.slice/chatons-herd-claude-831000.scope`.
   That is consistent with D174, which says deliberately that Herd copies
   no slice machinery and has no slice of its own on this host — the
   decision is about state, not supervision — but it means the sentence
   "Herd's sessions keep their state in Herd's own root" is true of the
   state root and not of the cgroup the session lives in. Nothing to fix
   here; worth not discovering later.
3. **ORCH-068: the launcher this project ships is not the launcher its
   sessions use.** This session runs from `/mnt/ssd_980/herd`, so the
   host half of D174 is already in force — `CLAUDE_CONFIG_DIR` is
   `/mnt/ssd_980/herd/claude/config` and no Herd session reads chatOnS's
   `settings.json` any more. But it was started by the root's own
   20-line `/mnt/ssd_980/herd/claude/env.sh`, not by the repository's
   87-line `ops/agents/env.sh`, and the two disagree on where the root
   is: the live one sets `CARGO_HOME=/mnt/ssd_980/herd/claude/cargo`
   (which exists), the shipped one and `state-root.md` say
   `/mnt/ssd_980/herd/cargo` (which does not). Sharpest consequence: the
   live launcher's line 12 is `export GH_CONFIG_DIR=/home/server/.config/gh`
   and the shipped one says nothing about `gh`, so a session started by
   `ops/agents/session.sh` would have `gh` read
   `$XDG_CONFIG_HOME/gh` — `/mnt/ssd_980/herd/claude/home/.config/gh`,
   which does not exist — and an agent started that way could not open or
   merge a PR. Nothing in the live root was changed to find this out.
4. **`gotchas/shared-config-dir.md` now reads as false.** Its "Expected
   now" still describes the clean fix as the owner's and outside the
   repository. ORCH-067 carries it; it was not rewritten here, because a
   gotcha sheet is its author's and a dated resolved line is enough.
5. **The brief's reading of `main` was two days and nine PRs stale.**
   Nothing broke because of it — the extra PRs touched the conflicted
   file at the same place — but the base commit in a brief is worth
   re-reading rather than trusting.

## Decisions needed

- **ORCH-068 — which launcher is authoritative, the repository's
  `ops/agents/` pair or the root's own scripts?** Recommendation: the
  repository's, with two corrections first (the `cargo` level and a
  `GH_CONFIG_DIR` the shipped script sets or deliberately drops), since a
  launcher nobody starts a session with is a launcher nobody keeps true.
  Fallback: keep the root's scripts as the real ones and let
  `state-root.md` describe them, which costs a one-directory correction
  to the page and to `env.sh`. Alexa's, because the root is hers.
- **ORCH-066 — the credentials sync unit**: Alexa's, outside the
  repository.
- ORCH-069 is sizing, not an owner's call: `herd-planner`.

## Resume point

PR #72 is merged and the branch is in `main`. Next, in order: ORCH-067
(one prose line in `CLAUDE.md` plus the gotcha's resolved line, a
`herd-step` task with `check-docs.sh` only), then ORCH-069 to
`herd-planner`, then put ORCH-068 and ORCH-066 to Alexa. PR #85 and PR
#67 are untouched and still conflicting; one integration at a time, and
#67's fate is Alexa's word.

## Processes

The gates ran as foreground and background `nix develop` commands in this
worktree and all exited on their own; nothing was killed, no PID was
signalled, and no `pkill`, `killall` or kill by name was used. Two
worktrees were created under this agent's own scratchpad directory,
`merge-72/wt` (the branch) and `merge-72/main-check` (detached at
`c1d5ce7`, for the pristine-`main` run); the second is removed at the end
of this task and the first is left for the record.
