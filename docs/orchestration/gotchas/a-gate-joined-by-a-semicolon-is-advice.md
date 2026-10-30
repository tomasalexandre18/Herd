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
  The costume turned up a second time the same night, in a fifth agent's
  `cargo fmt --all --check 2>&1 | tail -20 && echo "FMT OK"`, which printed
  `FMT OK` over two real rustfmt diffs; that agent also caught it by
  reading the output rather than the status, ran `cargo fmt --all`, and
  re-ran with the status preserved in a file.
- **And the costume defeated form 2 below, which is why that form is now
  stated with its conditions.** A sixth agent, on M12's P2a the morning
  of 2026-10-30, ran `cargo fmt --all --check 2>&1 | head -40` and
  reported `FMT EXIT: 0` read from `${PIPESTATUS[0]}` — over **four real
  rustfmt diffs printed in that very output**. Re-run bare, the gate was
  genuinely red. So form 2, which this file had recommended as the way
  to keep a status through a pipe, produced a clean zero over a red gate.
  Its author's diagnosis was *"`head` closing the pipe is enough to lose
  it; `${PIPESTATUS[0]}` is not a cure when the consumer closes the
  pipe."* **That diagnosis names the wrong cause**, and the difference
  decides which of the two defects to guard against, so it was measured
  on 2026-10-30 rather than reasoned about (fixture and figures in the
  third *Verify* below). Two separate faults live here, and only one of
  them can print a zero:
  - **A consumer that exits early changes what the producer's status
    means.** `head -40` closes the pipe at line forty and the producer
    dies of `SIGPIPE` on the bytes it had left to write, so
    `${PIPESTATUS[0]}` reads **141**, not the gate's verdict. Measured
    141 on five of five trials. 141 is wrong but it is *loud*: it is
    non-zero, so it cannot turn a red gate green. It can turn a green
    gate red, which costs an agent a re-run and nothing worse.
  - **A read taken one command too late prints a zero.** `PIPESTATUS` is
    replaced by the **very next** command, so an array read after
    anything at all describes that thing instead of the pipeline.
    Measured **0** every time. This is the fault that produced the
    reported transcript: `SIGPIPE` alone yields 141, and no measured
    arrangement of an early-exiting consumer yields 0. The giveaway is
    in the array's own shape — a two-stage pipeline leaves **two**
    elements, and a clobbered `PIPESTATUS` has exactly **one**.
- **Why the pipe is worse than the semicolon.** With `;` the refusal is
  still on the screen: a reader who looks can see that a gate said no
  above a phase that started anyway. A pipe loses the status *and* can
  cut the evidence off the top of the output — `tail -60`, `head -40`, a
  `grep` for one figure — so the transcript is free to show a number and
  a zero and nothing wrong at all. There is then nothing left for a
  reader to notice. That is the shape that reports a green over a red
  run, which is the one outcome this project does not accept at any
  strength (invariant 5; D27).
- **Expected now, wherever a gate's output is piped.** The same three
  forms, in the same order to prefer them — but the order stopped being
  a matter of taste on 2026-10-30. Forms 2 and 3 are **conditional**,
  and both of their conditions are things a one-line shell call makes
  easy to break without any sign that it broke. Form 1 carries no
  condition at all. Prefer it not because it is tidier but because it is
  the only one of the three that cannot be lost silently.

  1. **Do not pipe the gate at all.** This is the recommendation, and
     it is usually free: both gates already write the output to a file,
     so truncating their stdout in order to read it buys nothing.
     `./scripts/test.sh` tees cargo's whole stdout to `$HERD_CARGO_OUTPUT`,
     default `target/herd-cargo-output.txt` (`scripts/test.sh:90`), and
     `./scripts/gate.sh` writes its log to `--out FILE` or, with no flag,
     to a dated receipt named for the sha (`scripts/gate.sh:160`). Run
     the gate plain, read `$?`, then read the file. Unpiped, there is no
     early-exiting consumer to redefine the status and no array to
     clobber: `$?` is still the previous command's and nothing else's.
  2. **Take the gate's own status, not the pipeline's**, with
     `${PIPESTATUS[0]}` in bash — **and only under two conditions, both
     of which a gate command of this project was measured to break on
     2026-10-30.**
     - **Read it immediately**, in the very next expansion after the
       pipeline, because the *next command of any kind* replaces the
       array. Measured to clobber it: an `echo` on its own line before
       the read, a `[` test, a `( )` subshell wrapped round the
       pipeline, and even a plain `var=...` assignment. Measured **not**
       to clobber it: an `echo` that *carries* the read, since the
       expansion happens before `echo` runs; a `{ }` group; and an
       `if ... fi` round the pipeline, which is why `test.sh` may read
       it on the line after its `fi`.
     - **Do not pipe into a consumer that exits early.** Under
       `head -n`, `grep -q`, `grep -m` or a pager, `${PIPESTATUS[0]}` is
       not the gate's verdict at all but **141** — the producer killed
       by `SIGPIPE`. Form 2's own promise, "the gate's own status", is
       false in that arrangement however promptly it is read. It holds
       for a consumer that reads to EOF: `tee`, `cat`, `tail` and `wc`
       do, and `head` does not.

     `test.sh` is the worked example in this tree, and it is one because
     it meets **both** conditions, not just the first: it pipes cargo
     through `tee`, a consumer that reads to EOF, and takes
     `status=${PIPESTATUS[0]}` on the line directly after `fi` with
     nothing in between, above a comment saying that `tee`'s status "is
     not the suite's verdict" (`scripts/test.sh:149-160`, re-verified
     line for line on 2026-10-30). Copying its *shape* is safe; copying
     the idea while substituting `head` for `tee` is the defect above.
  3. **`set -o pipefail`**, where the shell honours it — bash and zsh do,
     `dash` and POSIX `sh` do not. Three caveats, and they are why it is
     third: these scripts deliberately do without `set -e`; a
     `pipefail` set in one one-line shell call does not survive to the
     next one, because a one-line shell call is not a script; and
     **`pipefail` does not rescue a late read** — measured 2026-10-30,
     with `pipefail` set and one intervening `echo`, both
     `${PIPESTATUS[0]}` and `$?` reported that `echo` and printed 0 over
     a red producer. `pipefail` also converts the `SIGPIPE` race into a
     failed pipeline rather than a wrong number, which is why two
     scripts here refuse `printf | grep -q` by name and read a
     here-string instead (`scripts/check-orchestration.sh:73` and
     `scripts/githooks/pre-push:272`).

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

  And the two arrangements that look like form 2 and are not, both of
  them taken from real gate commands of 2026-10-30:

  ```sh
  # WRONG, and this is the P2a defect: the label echo runs first, so the
  # array read describes that echo. Prints a clean 0 over a red gate.
  nix develop --command cargo fmt --all --check 2>&1 | head -40
  echo "--- fmt done ---"
  echo "FMT EXIT: ${PIPESTATUS[0]}"      # 0, whatever the gate said

  # WRONG a second way, even read immediately: head closes the pipe at
  # line 40, so this is the producer's death and not the gate's verdict.
  nix develop --command cargo fmt --all --check 2>&1 | head -40
  echo "FMT EXIT: ${PIPESTATUS[0]}"      # 141

  # Right, if it must be piped at all: a consumer that reads to EOF, and
  # the read in the same expansion. Prints the gate's own verdict.
  nix develop --command cargo fmt --all --check 2>&1 | tail -40
  echo "FMT EXIT: ${PIPESTATUS[0]}"      # the gate's status
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
  **And on this host a comparison that never ran is as silent as a status
  nobody looked at:** there is no `bc` on this PATH, so a load-gated wait
  loop written with `bc -l` prints `bc: command not found` on every
  iteration, never advances its counter and idles its full cap — batch
  14's pre-gate wait spent its whole 25 minutes that way at one-minute
  loads of about 2.0 to 2.9
  (`docs/orchestration/history/2026-10-30-merge-batch-15.md` *Risks and
  findings* 4), readings its own condition would have cleared within a
  minute, while `scripts/gate.sh` uses `awk "BEGIN{exit !(...)}"` for
  exactly that comparison at `:278`, `:291` and `:371`.
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
- **Verify, form 2's two conditions:** a producer that stands in for a
  red `cargo fmt --all --check` — it prints a diff and exits **1** — fed
  to `head -40`, with the status read in both positions. No cargo is
  needed to measure this and none was run; the producer is four lines of
  `sh`. Write two of them, one emitting its diff line by line and one
  emitting it in a single `write`, because the difference decides
  whether `SIGPIPE` fires at all:

  ```sh
  # producer-small.sh: 100 printfs, 1292 bytes, exits 1.
  i=1; while [ "$i" -le 100 ]; do printf 'diff line %d\n' "$i"; \
      i=$((i+1)); done; exit 1
  ```

  Measured on 2026-10-30 in a worktree of `a8cf397`, bash 5.3.9, five
  trials per cell, every cell stable across its five:

  ```text
  consumer   producer writes   read position        PIPESTATUS[0]
  head -40   line by line      immediately          141   (SIGPIPE)
  head -40   line by line      after one echo         0   (the echo)
  head -40   one write(2)      immediately            1   (the truth)
  head -40   one write(2)      after one echo         0   (the echo)
  cat        line by line      immediately            1   (the truth)
  none (bare, no pipe at all)                         1   (the truth)
  ```

  Read the table by column, because each column is one of the two
  conditions. **Read position** is the only input that ever produces a
  **0**: every late read printed 0 and no early read ever did, which is
  why the P2a transcript's `FMT EXIT: 0` is a late read and not
  `SIGPIPE`. **Consumer** and **write granularity** together decide
  whether the early read is the verdict (**1**) or the producer's death
  (**141**): `SIGPIPE` needs a consumer that exits early *and* a
  producer still holding bytes, so a producer that delivers its whole
  diff in one buffered `write` under the 64 KiB pipe buffer escapes it
  and a shell loop of separate `printf`s does not. A real gate's
  granularity is not an agent's to know, so treat 141 as always
  possible under `head`.

  Three candidate causes were tested and **ruled out**, each with the
  signature that rules it out, so that a seventh sighting is not
  re-diagnosed from scratch: `nix develop --command` swallowing its
  command's status (it propagates it — `nix develop --command sh -c
  'exit 7'` reports 7); the pipeline and the read landing in two
  *separate* shell invocations (a fresh shell's `PIPESTATUS` is empty,
  so the label prints with **nothing** after it, not a 0); and the
  command running under `sh`/`dash` (no `PIPESTATUS` at all, same empty
  expansion). None of the three can print a zero.
- **Replaces:** nothing. It is the missing half of
  `build-weight-off-root.md`, which says to run the preflight and does not
  say how to join it to what it guards.
