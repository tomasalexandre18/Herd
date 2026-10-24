# orchestration-corpus-cd — handoff 2026-10-24

- **Item:** none on the board (brief from the orchestrator)
- **Agent:** herd-step
- **Branch / commit:** fix-orchestration-corpus-cd / see `git log`, based
  on origin/main 0a2cf73
- **PR:** none (the contract forbids herd-step opening one)

## Done
`scripts/check-orchestration.sh` can no longer write outside its corpus.
The script runs without `-e`, so the unguarded `cd "$tmp/corpus"` let the
relative writes after it (decisions.md, `git add -A`, `commit`, `checkout`)
land in the current repository. Now `mkdir && cd` is fatal, and so is the
rest of the setup chain. `cd "$root"` after the section is fatal too.

Why not a sub-shell: `ok` and `bad` increment `pass` and `fail`; in a
sub-shell those counts are lost and the final `[ "$fail" -eq 0 ]` could be
green with failures inside. The fatal guard closes this occurrence and the
setup chain, which is what decides where the script stands.

Before/after, in throwaway clones, with a `BASH_ENV` file that makes
`cd */corpus` fail:
- before: HEAD 0a2cf73 -> 13de2b4 on a new branch `b`, decisions.md 8363
  -> 21 lines, three stray branches and commits;
- after: `FATAL: cannot enter .../corpus; touching nothing`, exit 1, HEAD,
  branch and decisions.md unchanged, working tree clean.

## Not done, and why
Nothing beyond the brief. The unknown cause of the original missed `cd`
was not investigated, as asked.

## Gates
- check-orchestration.sh: 87 passed, 0 failed
- check-docs.sh: docs ok (note: D170 gap, expected)
- test.sh: PASSED, 746 passed, 0 failed; the flaky `slots::` test did not
  fail
- clippy -D warnings: rc 0; fmt --check: rc 0
Skips, copied as skips (D27): 12 HERD-SKIPPED = 10 cgroup v2 delegation
unavailable + 2 ZFS io.max (`a_ceiling_this_host_can_carry...`,
`a_declared_ceiling_changes_...`).

## Risks and findings
- The brief asked for a PR; `herd-step.md` says no PR. The contract won.
- Other relative-path writes after a failed `cd` would need the same
  guard; none found in this script. `hook()` lines only run hooks.
- A `git init` inside a TMPDIR that sits within a repository is harmless
  now: the cd is checked and `git init` makes the corpus its own repo.

## Decisions needed
none

## Resume point
`herd-merger`: open the PR from this branch and integrate when CI is green.

## Processes
none left; one helper wait loop (PID 1583557) started and killed.
