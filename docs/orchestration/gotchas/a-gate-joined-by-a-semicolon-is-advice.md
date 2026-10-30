# A status you did not look at is a status you did not get

Filed as `a-gate-joined-by-a-semicolon-is-advice.md`, which is the name
every citation of this lesson uses and the name it keeps. The title
widened on 2026-10-30, the same day, when the same defect turned up
wearing a pipe instead of a semicolon.

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
- **Second costume, found the same day: a pipe discards a status exactly
  as `;` does.** A fourth agent's first gate command was
  `./scripts/test.sh | tail -60`. The shell reported **exit 0**. The
  suite's own last lines, in that very output, said
  `test suite: FAILED (cargo test exited 101)`. A pipeline's status is
  the status of its **last** command, and `tail` succeeded perfectly at
  tailing a red run. The agent re-ran it with the status preserved
  before concluding anything, and its own sentence names the costume:
  *"a pipe discards a status exactly as `;` does."* Found 2026-10-30
  while wiring the hub's pack frames.
- **Why the pipe is worse than the semicolon.** With `;` the refusal is
  still on the screen: a reader who looks can see that a gate said no
  above a phase that started anyway. A pipe loses the status *and* can
  cut the evidence off the top of the output — `tail -60`, `head -40`, a
  `grep` for one figure — so the transcript is free to show a number and
  a zero and nothing wrong at all. There is then nothing left for a
  reader to notice. That is the shape that reports a green over a red
  run, which is the one outcome this project does not accept at any
  strength (invariant 5; D27).
- **Expected now, wherever a gate's output is piped.** Three forms, in
  the order to prefer them:

  1. **Do not pipe the gate at all.** This is the recommendation, and
     it is usually free: both gates already write the output to a file,
     so truncating their stdout in order to read it buys nothing.
     `./scripts/test.sh` tees cargo's whole stdout to `$HERD_CARGO_OUTPUT`,
     default `target/herd-cargo-output.txt` (`scripts/test.sh:90`), and
     `./scripts/gate.sh` writes its log to `--out FILE` or, with no flag,
     to a dated receipt named for the sha (`scripts/gate.sh:160`). Run
     the gate plain, read `$?`, then read the file.
  2. **Take the gate's own status, not the pipeline's**, with
     `${PIPESTATUS[0]}` in bash. `test.sh` is the worked example in this
     tree: it pipes cargo through `tee` and then takes
     `status=${PIPESTATUS[0]}`, above a comment saying that `tee`'s
     status "is not the suite's verdict" (`scripts/test.sh:149-160`).
  3. **`set -o pipefail`**, where the shell honours it — bash and zsh do,
     `dash` and POSIX `sh` do not. Two caveats, and they are why it is
     third: these scripts deliberately do without `set -e`, and a
     `pipefail` set in one one-line shell call does not survive to the
     next one, because a one-line shell call is not a script.

  ```sh
  # Best: no pipe. Read the status, then read the log the gate wrote.
  nix develop --command ./scripts/test.sh
  echo "suite exited $?"   # then read target/herd-cargo-output.txt

  # If it must be piped, read the gate's status and not the pipeline's.
  nix develop --command ./scripts/test.sh | tail -60
  echo "suite exited ${PIPESTATUS[0]}"

  # pipefail: inside one bash script, not across two shell calls.
  set -o pipefail
  nix develop --command ./scripts/test.sh | tail -60 || echo "SUITE RED"
  ```

- **The general rule behind both costumes**, which is the part worth
  keeping when the third costume turns up: **a status you did not look
  at is a status you did not get.** `;` throws it away, a pipe replaces
  it with the last stage's, a subshell hands back the subshell's, `&`
  hands back nothing until that PID is waited for, and a `||` written to
  keep a transcript tidy swallows it outright. Each of them loses it
  differently and none of them says so. Before any verdict or figure is
  reported — in a handoff, on the board, to the orchestrator — the
  status it rests on has to have been printed on purpose.
- **Verify, the second costume:** the same fixture as above, now through
  a pipe, so the loss is shown and not asserted. A free-space floor no
  volume can meet makes the refusal certain, and `cat` is the most
  harmless pipe there is:

  ```sh
  HERD_PREFLIGHT_MIN_FREE_GIB=999999 ./scripts/bench-preflight.sh --quiet \
      | cat; echo "shell reported $?"
  HERD_PREFLIGHT_MIN_FREE_GIB=999999 ./scripts/bench-preflight.sh --quiet \
      | cat; echo "gate reported ${PIPESTATUS[0]}"
  ```

  Both were run on 2026-10-30 in a worktree of `a15b52d`. Each printed
  the free-space refusal and then
  `bench-preflight: REFUSED, do not start the heavy phase.`, and then:

  ```text
  shell reported 0
  gate reported 1
  ```

  One pipeline, one choice of where to read the status, opposite
  verdicts. The same line unpiped reports 1, and `set -o pipefail` ahead
  of the pipe reports 1; only `$?` after the pipe reports a clear that
  nothing cleared. The refusal itself is on stderr, so `cat` never even
  touches it: in this fixture the status is the *only* thing the pipe
  destroys, which is the half of the defect that no amount of careful
  reading can catch.
- **Replaces:** nothing. It is the missing half of
  `build-weight-off-root.md`, which says to run the preflight and does not
  say how to join it to what it guards.
