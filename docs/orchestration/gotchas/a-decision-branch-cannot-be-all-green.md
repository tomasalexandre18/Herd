# A branch that writes a decision cannot have four green gates

- **Date:** 2026-10-25 (C0b integration; raised independently by the two
  `herd-step` agents on `fix-false-success` and `identity-ran-commit`).
- **Observed:** both agents were briefed to deliver with the four gates
  green. Both delivered with `check-docs.sh` **red**, and both said so
  rather than hiding it. Neither was at fault: the gate was doing its job.
- **Why:** the two rules meet head-on. `docs/decisions.md` is written by a
  branch with the placeholder header `## <date> — D-NEXT <title>`, because
  only `herd-merger` allocates a number (invariant 8, D148) and two
  branches picking a number themselves collide — that is
  `decision-number-collisions.md`. And `check-docs.sh` **fails on a
  leftover `D-NEXT`** by design (`scripts/check-docs.sh`, the `next=`
  check), so that a placeholder can never reach `main`. A branch carrying
  a decision therefore fails that gate from the moment it writes the entry
  until the moment it is integrated. There is no ordering that avoids it.
- **Expected now:** do not brief "all four gates green" for a task that
  writes a decision. The deliverable is **three green gates plus a
  `check-docs.sh` whose only complaint is the `D-NEXT` placeholder**, with
  the output pasted so that the single remaining complaint is visible. An
  agent that reports this is complete, not blocked, and must not be sent
  back to make it green — the only way to do that is to allocate a number,
  which is exactly what an agent may not do.
- **Verify:** on the branch, `./scripts/check-docs.sh` prints the
  `still has a D-NEXT placeholder` note and nothing else, and exits
  non-zero; the other three gates are green. After integration, the
  merger's own run is fully green, with gap notes only for numbers held by
  open PRs. On this pass: red on both branches with that one note, green
  after D175 and D176 were allocated, with notes for D170 (PR #67) and
  D174 (PR #72).
- **Also allocate the number in the code.** `check-docs.sh` reads only
  `docs/decisions.md`, so `D-NEXT` written in a Rust comment passes every
  gate and ships as a dangling reference. This pass found **38** of them
  across 16 files. The merger greps `D-NEXT` over the whole tree, not just
  the decisions file, and attributes each line to the branch that added it
  before replacing it.
- **Replaces:** nothing written down. It corrects how the orchestrator
  briefs a decision-writing task, and it is a contradiction in the brief,
  not in the work that came back.
