# orch-record-fixes — handoff 2026-10-27

- **Item:** ORCH-112 (new); corrects ORCH-111's row and the handoff
  `2026-10-27-merge-batch-docs-gate-orch-batched-pr.md`
- **Agent:** herd-step
- **Branch / commit:** `orch-record-fixes` / `c0eb015` carries the four
  prose changes and this handoff; a second commit only adds this sha to
  the line you are reading. Based on
  `origin/main` `64cc532` (re-read in my own worktree, not taken from
  the brief, which named the same sha)
- **PR:** none — an agent pushes its branch and opens none (one PR per
  batch, `docs/orchestration/workflow.md`); `herd-merger` names it here

## Done

Prose only. Four files, no code, no `target/` created.

### 1. ORCH-111 no longer claims more than was measured

The row and the merge handoff both said the flaky case *"fails the same
way on `origin/main`'s copy"*. The diff does not support that. What the
diff supports is that the case is untouched by the batch that found it,
which makes the failure **not a regression of that batch** and says
nothing about whether it reproduces on pre-batch `main`. The suite was
not run there, where a run cost ~155 s on the old
`check-orchestration.sh` and a roughly 1-in-8 failure rate would have
needed many runs to be conclusive.

- `docs/orchestration/board.md`, ORCH-111: the claim is replaced by the
  one the evidence carries, the diff command that proves it is named in
  the cell, and the row now says in its own words that reproduction on
  `main` **was not measured** and must be re-measured. The sentence that
  read "this workstation's environment, not the code" is kept but marked
  as a reading of the captured output rather than a measurement, because
  the row's own next action can still end in a change to the case.
- the merge handoff: the original wording **stays**, and a dated
  correction bullet sits immediately after the bullet that holds it,
  naming what was written, that it was stronger than the measurement,
  and what the diff does prove. A handoff is evidence; nothing was
  rewritten in place.
- the row's next action now points at the new rule rather than only
  repeating "keep the whole output".

### 2. The rule that would have saved the thirty runs

`docs/orchestration/workflow.md`, section *Failure*, one bullet after
"A red gate blocks":

> **A gate's output goes to a file, and the file is what you read** —
> `./scripts/check-docs.sh > out.txt 2>&1`, never a pipe to `head`,
> `tail` or anything that truncates. A gate's output is the evidence,
> and an intermittent failure's first sample is the one you cannot get
> back: on 2026-10-27 the first two samples of ORCH-111 went into
> `tail -2`, and it took about thirty further runs to recover what they
> would have shown.

`docs/orchestration/templates/brief.md` carries the pointer and not a
second copy: its `Gates:` line now reads "each one redirected to a file,
never piped to head or tail (why: workflow.md, "Failure")".

**Why there.** *Failure* is where gate discipline already lives — the
red gate that blocks and the skip that is never a pass (D27) — and this
rule is the upstream half of the same requirement. The four existing
mentions of gate output (`templates/brief.md:26`,
`templates/handoff.md:23`, `.claude/agents/herd-step.md:22`,
`.claude/agents/herd-merger.md:36`) all say *report* it verbatim; none
says how to capture it, so there was nothing to extend, only a gap in
front of them. The brief's line is the one an agent reads before it
runs a gate, which is why that is the place for the pointer.

**Not in `CLAUDE.md`.** It is at exactly its 150-line limit, a line
added costs a line removed, and it holds no gate mechanics — it names
the four commands and delegates the procedure to `workflow.md`. I agree
with the orchestrator's read.

### 3. A board row for this task

`ORCH-112`, in *Owed*, state `done`, owner `herd-step` then
`herd-merger`. It records both corrections and carries the one finding
below that is not mine to fix.

## Not done, and why

- **The `check-docs.sh` fence anchor is reported, not touched.** The
  width rule toggles on a fence only at column 1, so a fenced block
  indented inside a list item is never seen as a fence and its lines are
  width-checked like prose. It has now caught two agents in their own
  handoffs. Loosening the anchor is a rule change (`workflow.md` step 6
  sends it to `herd-reviewer`), so it is a finding, not an edit. **It
  deserves a row**: the recommendation is in *Risks*.
- **No `docs/decisions.md` entry and no `D-NEXT`.** Writing down a rule
  that follows from D27 and from the existing verbatim-output
  requirement is not a decision; the brief asked for none.
- **No `docs/status.md` change.** No software state changed, and the
  file is at its 90-line limit.
- **No PR, no merge, no tag, nothing on the consumer's side** (D161).

## Gates

Prose only — `CLAUDE.md`, `docs/` and `.claude/` are the prose
allowlist, and this diff is four files under `docs/`. **The four Rust
gates were NOT RUN locally**; CI runs them on the batch PR this branch
joins. Nothing was compiled and no `target/` exists in my worktree.

Both scripts were redirected to a file and the file read in full — the
rule this branch writes.

```text
$ SP=/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/orch-record-fixes
$ ./scripts/check-docs.sh > "$SP/check-docs.out" 2>&1; echo "exit $?"
exit 0
$ cat "$SP/check-docs.out"
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```

```text
$ ./scripts/check-orchestration.sh > "$SP/check-orch.out" 2>&1; echo "exit $?"
exit 0
$ wc -l "$SP/check-orch.out"
153 .../check-orch.out
$ cat "$SP/check-orch.out"
... 141 `ok` lines read in full, no `bad`, no skip; the last line:
check-orchestration: 141 passed, 0 failed
```

The 141 `ok` lines include `PATH keeps what it inherited behind the four`
— ORCH-111's case — green in this run.

141 passed, 0 failed, as the brief expects; the D170 note is the known
gap and not a failure. **No skip**: neither script skips a case, and no
`HERD-SKIPPED` line was printed.

ORCH-111's intermittent case did not fire in this run. That is one green
observation of a case that is red about one run in eight, so it is not
evidence that the case is fixed.

## Risks and findings

- **The fence anchor deserves a board row, and my own files do not trip
  it.** Every fenced block I wrote — in this handoff — is at column 1,
  and the gate is green, so nothing of mine is caught. The row is worth
  opening anyway: the cost is a silent, misleading failure (the gate
  says "lines over 79" about a code block that is exempt by intent), it
  has hit two agents on consecutive days, and the fix is a rule change
  that wants a reviewer pass. A cheap middle option, if the anchor stays:
  `check-docs.sh`'s own header could say that a fence is only recognised
  at column 1, so the next author reads it before losing the ten
  minutes. I did not write that either — `scripts/` is outside my write
  paths and outside the prose allowlist.
- **Two more figures in ORCH-111's record are softer than they look, and
  I left them standing rather than inventing a number.** The merge
  handoff's *Risks* heading says the suite is red "about twice in
  thirty-two runs" while its *Gates* section says "about four times in
  thirty-two runs"; the board row and the brief say four. The run count
  to name the case reads "twenty-five runs" in the handoff and
  "thirty-odd runs" on the board row. Neither figure was recorded run by
  run. The correction bullet says so in the handoff, and the row keeps
  four, which is the figure its own *Gates* section measured.
- **One mechanism claim in the row is an inference, and is now labelled
  as one.** "Something inside `env -i … bash -c '. env.sh'` re-derived
  `PATH` from a profile" is a reading of the captured string — the
  empty-user `/etc/profiles/per-user//bin` tail built from the case's own
  `HOME=/wrong/home` — not an observation of the re-derivation. The
  handoff itself calls it "a lead, not a cause"; the row stated it flatly
  and now carries the same caveat. The front-four-prefixes-correct
  observation, and the four refuted hypotheses, are genuine measurements
  and are untouched.
- **"It has not been seen red on CI" is honest** as written: it is an
  absence of observation, stated as one, not a claim that CI is immune.
- **No risk to an invariant or a specification.** Nothing in this diff
  changes a gate, a protocol or a behaviour; the one rule added is a
  procedure for capturing output.

## Decisions needed

**None.** The one thing an owner could be asked — whether
`check-docs.sh`'s fence anchor is loosened — is a rule change for
`herd-reviewer` first, and it is not blocking anything.

## Resume point

Hand `orch-record-fixes` to `herd-merger` with the next batch; it
touches `docs/orchestration/board.md`, `workflow.md`,
`templates/brief.md` and one file in `history/`, so a branch that writes
a board row will conflict there and nowhere else. Then, if the fence
anchor is to be sized, open the row after ORCH-112 from *Risks* above.

## Processes

Started and ended by me: one `./scripts/check-docs.sh`, one
`./scripts/check-orchestration.sh`, and the `git` commands of this
branch. No PID left running; nothing killed by name, no `pkill -f`.

No `target/` to delete: nothing was compiled in
`…/scratchpad/orch-record-fixes/wt`, and the two gate outputs were
written to my scratchpad, outside the worktree.
