# A gate joined by `;` is advice

- **Date:** 2026-10-30 (three execution agents in one night, each of whom
  caught it, disclosed it and stopped).
- **Observed:** a heavy phase ran *after* `./scripts/bench-preflight.sh`
  had refused it. The command had been written as two statements on one
  line —
  `./scripts/bench-preflight.sh ; nix develop --command ./scripts/test.sh`
  — so the shell ran the second one whatever the first returned. Three
  branches in one evening. One of their authors wrote the rule in a
  sentence: *"a gate joined by `;` is advice, and I should have used
  `&&`."* Brief after brief says *"stop if it refuses — it is a gate, not
  advice"*; none of the three disagreed with that sentence. All three
  typed the wrong character between two commands.

  **Provenance, since the three are not equally quotable.** One is in the
  tree: `history/2026-10-30-audit-tests.md` *Processes* reports that its
  first heavy phase ran the preflight and the build in one `;`-separated
  command, so the build started although the preflight had **REFUSED** on
  a load of 11.36 against a ceiling of 9.00; that agent stopped the build
  and waited. The other two were disclosed to the orchestrator the same
  night from branches that are not ancestors of this one, and they are
  written down here on the orchestrator's account of them rather than
  quoted. Two agents the same night did it the other way and are worth
  reading for the shape that works:
  `history/2026-10-30-pack-registry-gate.md` waited for two consecutive
  clears over 24 attempts, and `history/2026-10-30-stale-comments.md`
  re-ran the preflight every 30 s through nine waits.
- **Why:** the cause is two characters wide and the effect is invisible.
  The refusal **prints** — `bench-preflight: REFUSED, do not start the
  heavy phase.` on stderr — and then the next command runs anyway, so the
  transcript of an *ignored* gate is byte-identical to the transcript of
  an *overruled* one: a refusal, then a build. Nothing says "this started
  despite the line above it". The exit status is the only place the
  difference lives, and `;` throws it away. `set -e` would not help
  either: the scripts here deliberately do without it (see the script's
  own note on style), and an agent's one-line shell call is not a script.
- **Expected now:** every preflight is joined to the phase it guards with
  `&&`, in **one** command, never `;` and never two separate tool calls:

  ```sh
  ./scripts/bench-preflight.sh && nix develop --command ./scripts/test.sh
  ./scripts/bench-preflight.sh --quiet && nix develop --command \
      cargo test -p herd-hub --lib
  ```

  Two separate calls are the same defect with more whitespace: the second
  one has no access to the first one's exit status, so the decision to
  start falls back on the agent having read the output — which is exactly
  the reading that failed three times. The same holds for a phase guarded
  by any other check, and the script's own header shows the shape:
  `./scripts/bench-preflight.sh && nix develop --command ./scripts/test.sh`.
- **And a clear is not a licence, however it is joined.** The preflight's
  closing note says *"all four figures are of this instant only"*: nothing
  watches the host after that line, and a phase cleared at a quiet moment
  has met load 29 and a full slice mid-suite. So the margin matters as
  much as the join — read the figures again before reading a red diff,
  and take a clear with a thin margin as a reason to wait rather than as
  permission. Never raise a floor to obtain a clear:
  `HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB` is not an agent's to set.
- **Verify:** a fixture that really refuses, so the check is proved to
  fire rather than trusted for its silence. A free-space floor no volume
  can meet makes the refusal certain, and it is not one of the two figures
  a heavy phase is actually judged on here:

  ```sh
  HERD_PREFLIGHT_MIN_FREE_GIB=999999 ./scripts/bench-preflight.sh --quiet \
      && echo "THE PHASE WOULD HAVE STARTED"
  ```

  With `&&` the refusal is the whole output. With `;` in its place the
  same refusal is followed by `THE PHASE WOULD HAVE STARTED`, which is
  what the three heavy phases did. Both halves were run on 2026-10-30 and
  behaved exactly so.
- **Replaces:** nothing. It is the missing half of
  `build-weight-off-root.md`, which says to run the preflight and does not
  say how to join it to what it guards.
