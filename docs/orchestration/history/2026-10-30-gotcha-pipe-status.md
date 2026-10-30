# gotcha-pipe-status — handoff 2026-10-30

- **Item:** none allocated by the brief; adjacent to ORCH-229, which is
  the open row for making `bench-preflight.sh` hard to ignore. This task
  is prose only and changes no script, so ORCH-229 stays open.
- **Agent:** herd-step
- **Branch / commit:** `gotcha-pipe-status` / `93d0901`, based on
  origin/main `a15b52d`; this handoff is the commit after it
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

`docs/orchestration/gotchas/a-gate-joined-by-a-semicolon-is-advice.md`
grew from 77 to 182 lines. The semicolon half — Date, Observed with its
provenance paragraph, Why, Expected now, *And a clear is not a licence*,
and its fixture Verify — is byte-identical to what landed six hours ago.
What is new:

- **The title**, now *A status you did not look at is a status you did
  not get*, with a short note under it that the filename does not change
  because every citation of this lesson is by path: ORCH-229 on the
  board, `history/2026-10-30-merge-batch-13.md`, and
  `history/2026-10-30-foreign-and-quiet.md` all cite
  `gotchas/a-gate-joined-by-a-semicolon-is-advice.md` and none cites the
  heading text. Checked with `grep -rn` over `*.md` and `*.sh` before
  retitling.
- **The pipe costume**, with the instance: a fourth agent's first gate
  command was `./scripts/test.sh | tail -60`, the shell reported exit 0,
  and the suite's own last lines in that same output said
  `test suite: FAILED (cargo test exited 101)`. Found 2026-10-30 while
  wiring the hub's pack frames. A pipeline's status is its last
  command's, and `tail` succeeded at tailing a red run.
- **Why it is worse than `;`**: with `;` the refusal stays on the screen
  for a reader to notice; a pipe loses the status *and* can cut the
  evidence off the top of the output, so a transcript may show a figure
  and a zero and nothing wrong. That is the shape that reports a green
  over a red run, against invariant 5 and D27.
- **Three correct forms, in preference order, with the recommendation
  stated.** First and recommended: do not pipe a gate at all, because
  both gates already write the output to a file —
  `scripts/test.sh:90` tees cargo's whole stdout to `$HERD_CARGO_OUTPUT`,
  default `target/herd-cargo-output.txt`, and `scripts/gate.sh:160`
  writes its log to `--out FILE` or to a dated receipt named for the sha.
  Second: `${PIPESTATUS[0]}`, citing `scripts/test.sh:149-160`, which is
  this tree's own worked example — it pipes cargo through `tee`, takes
  `status=${PIPESTATUS[0]}`, and says in a comment that `tee`'s status
  "is not the suite's verdict". Third: `set -o pipefail` where the shell
  honours it, with both caveats — these scripts deliberately do without
  `set -e`, and a `pipefail` set in one one-line shell call does not
  survive to the next, because a one-line shell call is not a script.
  The three line numbers were read out of the scripts on this branch,
  not recalled.
- **The general rule**, as the thing to keep when the third costume
  appears: a status you did not look at is a status you did not get.
  `;` throws it away, a pipe replaces it with the last stage's, a
  subshell hands back the subshell's, `&` hands back nothing until that
  PID is waited for, and a `||` written to tidy a transcript swallows it
  outright.
- **A second Verify that fires**, in the file's existing form: the same
  impossible free-space floor, now through `cat`, read twice. Run on
  this branch; the two outputs are quoted in the file and in the agent's
  report. It also records that `--quiet` puts the refusal on stderr, so
  `cat` never touches it and the status is the only thing that pipe
  destroys — which isolates the half of the defect no careful reading
  can catch.

`docs/orchestration/gotchas/README.md`: the one index row for this file
was rewritten to carry both costumes and the rule. The path in column one
is unchanged.

Diff: two files, `+109 -3`.

## Not done, and why

- **No script change.** Making the preflight hard to ignore is ORCH-229
  and is not prose; `scripts/*` was outside this task's ownership.
- **No `board.md`, `decisions.md` or `status.md` edit.** Outside
  ownership. A proposed amendment to ORCH-229's row is drafted below.
- **The semicolon half was not rewritten**, by instruction. Its
  provenance paragraph still says "three execution agents"; the count of
  agents who hit the family of defects in one night is now four, and that
  is stated in the new half rather than by editing the old one.

## Gates

Baseline, measured in the shared checkout at `a15b52d` **before** any
edit, so the comparison is this agent's own:

```text
$ ./scripts/check-docs.sh
docs ok
=== check-docs exit: 0 ===

$ ./scripts/check-orchestration.sh
check-orchestration: 324 passed, 0 failed
=== check-orchestration exit: 0 ===
```

After the amendment, before this handoff was written, in the worktree:

```text
$ ./scripts/check-docs.sh
docs ok
=== check-docs exit: 0 ===

$ ./scripts/check-orchestration.sh
check-orchestration: 324 passed, 0 failed
=== check-orchestration exit: 0 ===
```

No delta: 324 passed and 0 failed both times, `docs ok` both times. None
of the 324 cases reads this gotcha's text, and the only case family the
amendment could have moved is the width family — one over-long line in
`docs/` turns 13 cases red at once, seven sightings on this project.

The width rule was not trusted for its silence. `check-docs.sh:85-99`'s
own awk was run against the amended file (no finding), and then against a
copy of it with 90 `x` characters appended, which it reported as
`corrupt.md:172 (90 cols)`. The detector fires. No `sed` or `perl -pe`
touched any line of this file; every em dash was written through the file
tools, so there was no byte path on which a double-encoding could happen.

**The four Rust gates were NOT RUN** — `cargo build`, `./scripts/test.sh`,
`cargo clippy`, `cargo fmt --check`. This change is prose only: two `.md`
files under `docs/`, no `.rs` file, no `Cargo.toml`, no script. No
`cargo` was invoked at any point, by instruction and because the
heavy-agent ceiling of two is taken by two mergers while the first
consumer's hub and runner are live on this host. What covers them is the
batch's own `./scripts/gate.sh` run at integration, which is
`herd-merger`'s. A gate that did not run is reported as not run.

Last run, named and not quoted, on the committed tree after this file:
`./scripts/check-docs.sh` and `./scripts/check-orchestration.sh` in
`.../scratchpad/wt-gotcha`, both to stdout; the verbatim output is in the
agent's report. The branch is pushed only if both are green.

Skips: no skip. No `HERD-SKIPPED` line was produced or copied, because
the suite was not run.

## Risks and findings

- **The file now carries two Verify bullets.** It is a flat bullet list
  with one label per bullet, so the second is labelled *Verify, the
  second costume* rather than merged into the first. A reader scanning
  for "Verify" finds the semicolon's fixture first. Judged better than
  rewriting the one that is already cited.
- **One thing the file already covered that a reader may read as new.**
  The old half's *Expected now* already says: *"Two separate calls are
  the same defect with more whitespace: the second one has no access to
  the first one's exit status."* That is the same root cause as the pipe,
  one costume earlier, and it is the nearest thing to prior coverage in
  the file. What was **absent**: the words pipe, pipefail, `PIPESTATUS`,
  `tail`, `head` and `grep` appear nowhere in the pre-amendment file, nor
  anywhere else under `docs/orchestration/gotchas/` or in
  `workflow.md` — checked with a case-insensitive `grep -n` for those six
  terms over the whole gotchas directory and `workflow.md`, which
  returned nothing. So the pipe case was genuinely missing, and the
  brief's premise holds.
- **`set -e` is mentioned twice now**, once in the old *Why* and once in
  the `pipefail` caveat. Deliberate: the two say different things (why
  `set -e` would not have saved the semicolon case, and why `pipefail` is
  ranked third). Not deduplicated, since deduplicating would mean editing
  the half that is cited.
- **The pipe's instance is reported, not quoted from the tree.** The
  fourth agent's branch is not an ancestor of this one and its handoff is
  not in `docs/orchestration/history/`; the file says so in the same
  register the old half uses for its two unquotable agents. If that
  branch lands, its handoff becomes the citable provenance and this line
  can point at it.
- **ORCH-229's row understates the problem by one costume.** Its text
  says "three agents in one night" and names only the `;` join. It is
  outside this task's ownership; the amendment is drafted below.

## Drafted for `herd-merger`

No `docs/decisions.md` entry: the brief asked for none, and this is a
gotcha amendment, not a decision. **none** for `D-NEXT`.

One **proposed amendment to the existing ORCH-229 row** in
`docs/orchestration/board.md`. It is given field by field, not as a table
row, because a drafted row may not contain a literal pipe character —
the column counter is `awk -F'|'` and a backslash does not help it. The
merger or a later planner pastes these into the existing row's columns;
nothing else in the row changes.

- Column *Item*: `ORCH-229` — unchanged.
- Column *What* — replace the first sentence,
  "`bench-preflight.sh` cannot tell whether anybody acted on its exit
  status, and three agents in one night did not", with:
  "**No gate here can tell whether anybody acted on its exit status, and
  four agents in one night did not.** Three joined `bench-preflight.sh`
  to the heavy phase with `;` instead of `&&`, so the refusal printed and
  the phase started anyway. The fourth piped `./scripts/test.sh` through
  `tail -60` and read exit 0 over a suite whose own last lines said
  `cargo test exited 101`, because a pipeline reports its last command's
  status. The lesson covers both costumes
  (`gotchas/a-gate-joined-by-a-semicolon-is-advice.md`) and prose is
  still all that has been done."
  The rest of that column — the two mechanisms (a) and (b) and their
  dashes — is unchanged.
- Column *Evidence*: append `; the pipe instance, 2026-10-30, reported
  by the agent that caught it` after the existing text.
- Column *Owner*: `herd-planner` — unchanged.
- Column *State*: `open` — unchanged.
- Column *Notes* — append to the existing recommendation: "A `--run`
  mode, option (a), also closes the pipe costume, because an agent who
  never writes the join never writes a pipe around it either; option (b)
  does not, since a marker says nothing about how a status was read."
- Column *Links*: unchanged.

## Decisions needed

none. The one judgement the brief delegated — whether to retitle — was
taken: retitled, filename kept, index row updated, and the reason
(every citation is by path) is written into the file itself.

## Resume point

`git -C <worktree> log --oneline -2` on `gotcha-pipe-status`, then
`herd-merger` picks the branch up in the next batch. Nothing else on this
file is pending. The open successor is ORCH-229, which needs the planner
to choose mechanism (a) or (b) before anybody writes a script; the
drafted row above is the only input this task leaves it.

## Processes

- PIDs started and ended: none. No background process, no build, no
  test, no `cargo` invocation. The only binaries run were
  `./scripts/bench-preflight.sh` (read-only: it reads `/proc`, `df` and
  the cgroup files, and with an impossible floor it refuses and exits
  before doing anything), `./scripts/check-docs.sh`,
  `./scripts/check-orchestration.sh`, `git`, `grep`, `awk` and `cat`.
  All in the foreground, all short-lived. No `pkill`, no `killall`, no
  kill by name.
- Own `target/`: never created, because nothing was compiled.
  `CARGO_TARGET_DIR` was never set and `/mnt/ssd_980/herd/build/` was
  not written to. Nothing to delete after the push. The worktree at
  `.../scratchpad/wt-gotcha` holds two changed `.md` files and no build
  output.
