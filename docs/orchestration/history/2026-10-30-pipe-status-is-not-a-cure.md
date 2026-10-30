# pipe-status-is-not-a-cure — handoff 2026-10-30

- **Item:** ORCH-259 (drafted by this task; ORCH-258 was the highest row
  id anywhere, on the pushed and unlanded `m12-plan-rows`)
- **Agent:** herd-step
- **Branch / commit:** `pipe-status-is-not-a-cure` / see the agent's
  report, based on origin/main `a8cf397`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

The task began with a measurement and the measurement settled it.

**The mechanism is a late read, not `SIGPIPE`.** A fixture reproducing
the reported outcome was built in
`scratchpad/.../psnc/` — a four-line `sh` producer that prints a diff and
exits **1**, standing in for a red `cargo fmt --all --check`, fed to
`head -40`, with `${PIPESTATUS[0]}` read in both positions. No `cargo`
was run and none was needed. Five trials per cell, bash 5.3.9, every
cell stable across its five:

```text
consumer   producer writes   read position        PIPESTATUS[0]
head -40   line by line      immediately          141   (SIGPIPE)
head -40   line by line      after one echo         0   (the echo)
head -40   one write(2)      immediately            1   (the truth)
head -40   one write(2)      after one echo         0   (the echo)
cat        line by line      immediately            1   (the truth)
none (bare, no pipe at all)                         1   (the truth)
```

**Read position is the only input that ever produces a 0.** Every late
read printed 0; no early read ever did. So the reported `FMT EXIT: 0` is
an array read taken one command too late, and the P2a agent's own
diagnosis — *"`head` closing the pipe is enough to lose it;
`${PIPESTATUS[0]}` is not a cure when the consumer closes the pipe"* —
**names the wrong cause**. `SIGPIPE` is real, reproducible and also a
defect, but it yields **141**: non-zero, so it cannot turn a red gate
green. It can turn a green gate red, which costs a re-run and nothing
worse. The dangerous fault is the one that manufactures a zero.

Both candidate mechanisms in the brief therefore reproduce, and they are
**different defects with opposite blast radii**. That distinction is the
substance of the amendment.

Secondary measurements, all 2026-10-30, all on `a8cf397`:

- **What clobbers `PIPESTATUS`:** an `echo` on its own line before the
  read; a `[` test; a `( )` subshell wrapped round the pipeline; and a
  plain `var=...` assignment. A bare variable assignment is a command.
- **What does not:** an `echo` that *carries* the read (the expansion
  runs before `echo` does); a `{ }` group; and `if ... fi` round the
  pipeline, which is why `test.sh` may read it on the line after `fi`.
- **The giveaway is the array's length.** A two-stage pipeline leaves
  **two** elements; a clobbered `PIPESTATUS` has exactly **one**.
- **`set -o pipefail` does not rescue a late read:** with `pipefail` set
  and one intervening `echo`, both `${PIPESTATUS[0]}` and `$?` reported
  that `echo` and printed 0 over a red producer.
- **`SIGPIPE` depends on write granularity**, which is why the table has
  that column: a producer delivering its whole diff in one buffered
  `write` under the 64 KiB pipe buffer escapes `SIGPIPE` and reports the
  truth; a shell loop of separate `printf`s does not. A real gate's
  granularity is not an agent's to know, so 141 is always possible
  under `head`.

**Three further candidate causes were tested and ruled out,** each with
the signature that rules it out, so a seventh sighting is not
re-diagnosed from scratch:

- `nix develop --command` swallowing its command's status — it does not.
  `nix develop --command sh -c 'exit 7'` reports **7**. (This was worth
  testing because every real cargo call in this project carries that
  wrapper, and the brief's reported command did not show it.)
- the pipeline and the read landing in two *separate* shell invocations
  — a fresh shell's `PIPESTATUS` is empty, so the label prints with
  **nothing** after it, not a 0.
- the command running under `sh`/`dash` — no `PIPESTATUS` at all, same
  empty expansion.

None of the three can print a zero.

**The gotcha is amended in place, shape and three forms intact**
(`docs/orchestration/gotchas/a-gate-joined-by-a-semicolon-is-advice.md`;
six documents cite it by path and the path is unchanged, as is the
title):

- a new bullet records the sighting and splits the two faults apart,
  stating plainly that the P2a diagnosis names the wrong cause and why
  the difference decides which defect to guard against;
- **form 1 is promoted on measured grounds**, not stylistic ones: forms
  2 and 3 are conditional and their conditions are things a one-line
  shell call breaks without any sign that it broke, while form 1 carries
  no condition at all;
- **form 2 now states its two conditions** — read it immediately, in the
  very next expansion; and do not pipe into a consumer that exits early,
  because under `head -n`, `grep -q`, `grep -m` or a pager
  `${PIPESTATUS[0]}` is not the gate's verdict but 141. Form 2's own
  promise, "the gate's own status", is **false** in that arrangement
  however promptly it is read;
- form 3 gains a third caveat, that `pipefail` does not rescue a late
  read;
- the code fence gains the two arrangements that look like form 2 and
  are not, both from real gate commands, each with its measured value in
  a trailing comment;
- a third *Verify* bullet carries the fixture, the table and the three
  ruled-out causes.

**Every line-number citation in the file was re-verified rather than
trusted**, since the brief warned they had moved twice. All five are
exact on `a8cf397`: `scripts/test.sh:149-160` (comment at :149-154,
the two piped `cargo test` lines at :155-159, `status=${PIPESTATUS[0]}`
at :160), `scripts/test.sh:90`, `scripts/gate.sh:160`, and
`scripts/gate.sh:278`, `:291`, `:371` for the `awk "BEGIN{exit !(...)}"`
comparison. The `test.sh` citation is not merely present but **correct
on both of the conditions just measured**, which is the stronger claim
the amendment now makes for it.

## Not done, and why

- **No script was changed.** The sweep found nothing to change (below),
  and `scripts/` owes all four Rust gates, which this task does not run.
- **No `docs/decisions.md` entry and no `D` number.** The brief asked
  for neither, and the file is `herd-merger`'s alone.
- **`docs/status.md` untouched**, as instructed — it is at its 90-line
  ceiling with no slack.
- **The late-read half is left undetected.** ORCH-259 says why: it is
  probably not detectable statically, and it must not hold up the half
  that is.
- **No count of sightings is asserted in the gotcha.** The brief calls
  this "the fifth sighting of this family"; the file as landed already
  describes three semicolon sightings and two pipe sightings, which
  makes this one the sixth overall or the third of the pipe costume
  depending on how "family" is read. Rather than land an arithmetic
  disagreement into a file six documents cite, the new bullet dates the
  sighting and does not number it. Flagged under *Risks and findings*.

## Gates

Run **before** this file was committed, on the tree carrying the gotcha
amendment and the board row, verbatim:

```text
$ ./scripts/check-docs.sh
docs ok
check-docs exit: 0
```

Baseline runs on the untouched `a8cf397` worktree, for comparison:

```text
$ ./scripts/check-docs.sh
docs ok
check-docs exit: 0

$ ./scripts/check-orchestration.sh
check-orchestration: 324 passed, 0 failed
check-orchestration exit: 0
```

An over-79-column scan of the amended gotcha, using `check-docs.sh`'s
own column arithmetic under `LC_ALL=C` (bytes minus continuation bytes,
fenced blocks and table rows exempt), reported no line over 79.

**The four Rust gates were NOT RUN**, and not merely unreported: this is
a prose-only change, `cargo` was forbidden by the brief because two
heavy agents hold M12 tasks, and nothing outside `docs/` was touched.
What covers them is the batch's own `./scripts/gate.sh` run at
integration, which is `herd-merger`'s and not this task's.

**The last runs, named and not quoted**, both on the committed tree
carrying this handoff: `./scripts/check-docs.sh` and
`./scripts/check-orchestration.sh`, from the worktree root. Their output
is in the agent's report, not in the tree they gate.

**No skip.** Neither gate reports skips, and no `HERD-SKIPPED` line was
produced or copied anywhere in this task.

## Risks and findings

1. **The sweep of `scripts/` found nothing wrong, and the citation holds
   up.** Reported either way as the brief asked. Exactly **one**
   `PIPESTATUS` read exists in the whole directory, `scripts/test.sh:160`
   — plus the `$PIPESTATUS[0]` mention in its own comment at `:153` —
   and it satisfies both measured conditions: its consumer is `tee`,
   which reads to EOF, and the read is taken on the line directly after
   `fi` with no command in between. Measured, not inferred: a fixture of
   that exact shape (`if ... fi` round the pipeline, read after `fi`)
   returns the producer's status and a two-element array. No gate script
   is piped into an early-exiting consumer anywhere; `test.sh:156` and
   `:158` pipe cargo into `tee`, which reads to EOF. Every other script
   sets `-uo pipefail`.
2. **Two scripts already documented this exact `SIGPIPE` race, and
   avoid it by name** — `scripts/check-orchestration.sh:73-80` and
   `scripts/githooks/pre-push:272-280` both refuse `printf | grep -q`
   and read a here-string instead, with measured figures (200 runs per
   size, 4 KiB stdio buffer). The project knew the producer-death half
   of this defect in its scripts and had not carried it into the agent
   guidance. The amendment now cross-references both.
3. **One latent instance, benign, reported for completeness:**
   `scripts/check-docs.sh:302` is
   `version=$(sed -n '...' Cargo.toml | head -1)` under `set -uo
   pipefail`. `head -1` can close the pipe on `sed`, making the
   pipeline 141, but the assignment's status is never read and `set -e`
   is off, so the *value* is correct and nothing can bite. Not a gate,
   not a defect, **no fix proposed** — recorded so a future sweep does
   not re-open it as one.
4. **`check-orchestration.sh:339` contains the bad shape as a blessed
   fixture**, as the string
   `./scripts/check-docs.sh 2>&1 | tail -5` inside a `hook 0` assertion.
   That is a permissions test — it asserts `orchestrator-guard.sh`
   *allows* the orchestrator to read a gate's output — and pipe hygiene
   is not that hook's job today. Not a defect. It is, however, evidence
   for ORCH-259 that the shape is currently unremarkable to every
   automated reader in the tree.
5. **Sighting count, unresolved and deliberately not landed.** See *Not
   done*. If the orchestrator wants a number in the file, it needs a
   ruling on what "this family" counts, and the merger can paste one
   sentence.
6. **Something the orchestrator wrote is wrong, in those words, and it
   is a small one in the brief rather than in the gotcha.** The brief's
   leading candidate says `PIPESTATUS` is clobbered by the next command
   "including an `echo` or a `[` test — if anything ran between the
   pipeline and the read — **even the `echo` that printed the label**".
   Measured, the echo that *carries* the read is **safe**: parameter
   expansion happens before `echo` runs, so
   `cmd | head -40; echo "FMT EXIT: ${PIPESTATUS[0]}"` prints the
   producer's status (1, or 141 under `SIGPIPE`), never 0. Only an
   `echo` that *precedes* the read clobbers. The consequence is
   practical, not pedantic: the reported transcript **cannot** have come
   from the one-liner the brief quotes, so something else ran between
   the pipeline and the read in the P2a agent's session, and the
   original command as quoted is not sufficient to reproduce the
   reported 0. Whoever reads P2a's transcript should expect at least one
   command in between.
7. **And the thing the gotcha itself got wrong.** Its form 2 promised
   `${PIPESTATUS[0]}` gives "the gate's own status". Under a consumer
   that exits early it gives 141, which is neither the gate's verdict
   nor a zero — so the promise was false in precisely the case an agent
   reaches for a pipe, and the file never stated the immediacy condition
   that actually failed. Worse, its worked example used `tail -60`,
   which reads to EOF and therefore never triggers `SIGPIPE`: the
   example was **correct by accident of the consumer chosen**, so a
   reader copying the idea while substituting `head -40` lost the
   guarantee with no warning from the file. That is the shape of the P2a
   defect, and it is the gotcha's own doing.
8. **The amendment makes the file longer.** It was already the longest
   gotcha in the directory. The honesty of form 2 cost roughly 90 lines
   of prose, and a reader in a hurry reaches the three forms later than
   before. Mitigated by keeping the numbered forms in place and in
   order, so the shape a citation points at is unchanged, but worth an
   eye if the file grows again.

## Drafted for `herd-merger`

No `docs/decisions.md` entry: the brief asked for none, and nothing here
is a decision — it is a measurement and a correction to prose.

**One `board.md` row, already inserted on this branch** at the end of the
*Open* section after ORCH-252, and reproduced here in full because the
merger reads the tree and nothing else. **ORCH-258 is the highest row id
anywhere** (on `m12-plan-rows`, pushed and awaiting a landing; `main` is
at ORCH-252), so this took ORCH-259 — renumber if that branch lands
first or allocates further.

```text
| ORCH-259 | Nothing in the tree **detects** a gate whose stdout is handed to a consumer that exits early, and on 2026-10-30 the remedy the gotcha sheet recommended for exactly that case was measured to fail in two separate ways — one of them printing a clean `FMT EXIT: 0` over a red `cargo fmt --all --check` on M12's P2a. `scripts/` itself is **clean**: the sweep of 2026-10-30 found exactly one `PIPESTATUS` read in the whole directory, `scripts/test.sh:160`, and it satisfies both measured conditions (its consumer is `tee`, which reads to EOF, and the read is taken on the line directly after `fi` with no command in between). So this row is a **detector**, not a repair, and no script is owed a fix | sixth sighting of the pipe costume, M12 P2a, 2026-10-30; mechanism measured on branch `pipe-status-is-not-a-cure` | herd-step | open | Size a check that fires on the **shape** rather than on the remedy, because the remedy is what failed. The detectable half is an agent Bash command that runs one of the five gates and sends its stdout to `head -n`, `grep -q`, `grep -m` or a pager, and `.claude/hooks/orchestrator-guard.sh` already parses agent commands, so that is the cheap place to put it. The late-read half — an array read taken one command after the pipeline — is probably not detectable statically and must not hold the first half up. Two constraints for whoever sizes it: prove the detector fires on a known-bad fixture before trusting its silence, and note that `scripts/check-orchestration.sh` is under all four Rust gates while `.claude/` is prose-exempt, so the two halves may not belong in one task | — | `gotchas/a-gate-joined-by-a-semicolon-is-advice.md`, the *Verify, form 2's two conditions* bullet; `history/2026-10-30-pipe-status-is-not-a-cure.md` |
```

That cell carries **no literal `|`**, which the board's `awk -F'|'`
counter requires and which this task's subject made an active hazard:
every pipe in it is worded (`sends its stdout to`, `handed to`) rather
than typed. The *State* cell is the single word `open`, one of the
seven; all the prose is in *Next action*. Field count is 10 under
`awk -F'|'`, matching all 95 existing *Open* rows.

## Decisions needed

None blocking. One optional ruling, for the orchestrator and not for
Alexa, since it is a matter of how rather than what: **what counts as
one "family" for the sighting tally** (see *Risks and findings* 5).
Recommendation: leave the file as landed, dating sightings without
numbering them, because a count that disagrees with its own file is
worse than no count. Fallback: the merger pastes one sentence fixing
the tally when it lands.

## Resume point

Nothing is owed on this branch. The next step is `herd-merger` taking
`pipe-status-is-not-a-cure` into the current batch and allocating
ORCH-259 its final number. If the detector of ORCH-259 is wanted, brief
it as its own task against `.claude/hooks/orchestrator-guard.sh`, which
is prose-exempt, and keep any `scripts/check-orchestration.sh` half in a
separate task that runs all four Rust gates.

## Processes

PIDs started: none that outlived their command. Every process this task
ran was a short foreground one — `git`, `sed`, `awk`, `grep`, the
`sh` fixtures, `./scripts/check-docs.sh`,
`./scripts/check-orchestration.sh`, and one
`nix develop --command sh -c 'exit 7'` to measure status propagation.
Nothing was backgrounded, nothing was signalled, and no `pkill`,
`killall` or kill-by-name was used. The four untouchable instances were
never approached: the consumer's hub 3006543 and runner 3021002, and the
pinned v0.4.1 hub 565152 and runner 607643 on `127.0.0.1:8791`.

No `target/` to delete: no `cargo` ran, `CARGO_TARGET_DIR` was never
set, and this task compiled nothing. The worktree itself lives in this
task's own scratchpad subdirectory and holds only the checkout.
