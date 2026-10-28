# method-landing-record — handoff 2026-10-28

- **Item:** ORCH-NEXT (T4 of the local-merge method; the planner's plan
  reserves ORCH-125 and up — `herd-merger` allocates the row)
- **Agent:** herd-step
- **Branch / commit:** method-landing-record / 1a8a55c (content) plus
  the commit that adds this handoff, based on origin/main bc83717
- **Landing:** none — an agent pushes its branch and lands nothing;
  `herd-merger` names the landing here and fills the record

## Done

T4 of the local-merge method: the audit trail the pull-request body
carries today now has a home that survives the pull request going away.

1. **`docs/orchestration/templates/landing.md`** (new, 93 lines). The
   landing record is the **merge commit message** on `main`, written
   twice in the same push: as the commit message, and inside
   `herd-merger`'s own handoff, which is the copy a gate can read.
   Seven sections: the six of `.github/pull_request_template.md`,
   adapted, plus *CI run*.

   | pull_request_template.md | landing.md |
   |---|---|
   | Milestone and expectations | `## What changed and why` |
   | Branches in this batch | `## Branches in this batch` |
   | Evidence | `## Evidence` (gate verdict + sha + skips) |
   | Left out | `## Left out` |
   | To record | `## Decisions allocated` |
   | Handoff | `## Board rows` |
   | — | `## CI run` (new) |

   Four header fields carry the machine-checkable facts: `**Batch:**`,
   `**Gate:**` (verdict and the exact sha pushed, plus the receipt
   path), `**Skips:**`, `**CI run:**`. *CI run* says in the template
   itself that the run id is **recorded and not waited on** — the nine
   minutes are what the method buys back — and that its verdict is read
   at the next session start.

   Two adaptations away from the plan's §4 wording, both deliberate and
   both reversible in one `sed`: *To record* → *Decisions allocated* and
   *Handoff* → *Board rows*, because in a landing the decisions are
   already allocated (the merger allocates them) and "Handoff" reads
   wrong in a merge commit, where the per-branch handoffs are listed
   under *Branches in this batch*. The plan's §4 says "same sections";
   the brief says "adapted". I followed the brief and say so here.

   The template also drops the PR template's empty ```` ```text ````
   block: a commit message is plain text, and a nested fence inside the
   template's own fence does not pair under `check-docs.sh`'s
   fence rule (a fence closes only at the indent it opened on, so a
   second fence at column 0 closes the outer one). The verbatim output
   stays in the handoff, under its own fences, as today.

2. **`docs/orchestration/templates/handoff.md`**: `- **PR:**` is now
   `- **Landing:**`, and four lines of prose say that a
   `<date>-merge-<batch>.md` handoff also carries the filled landing
   record. Nothing else in the template changed.

3. **One per-file case in `scripts/check-docs.sh`** (+54 lines, no line
   removed): a handoff named `*-merge-*.md` whose filename date is on or
   after `landing_from=2026-10-28` must carry the record's seven
   sections and the four filled fields. Red-path output below.

   Shape, and why it is cheap: the date in the filename is compared as a
   shell string first, so the 21 merge handoffs written under the
   pull-request method are **skipped without being opened**, and what is
   left is read by **one** `awk` for the whole set, not one per file. No
   `basename` fork either (`${f##*/}`): 21 forks per run, paid 32 times
   by `check-orchestration.sh`, measured at 0.84 s of suite time before
   I removed them.

## Not done, and why

- **No case in `check-orchestration.sh`.** The brief gives that file's
  new sections to T1 (gate) and T5 (pre-push), and tells me to stay out.
  So the new rule's red path is demonstrated below but **not yet in the
  32× harness**, which is exactly the thing `check-docs.sh:5-11` says
  makes a control trustworthy. This is a finding, not a fix: one board
  row, after T1 and T5 have landed, for six `expect_red`/`expect_green`
  cases built from the fixtures in my scratchpad. Cost estimate: six
  more whole-corpus runs, 32 → 38, +1.6 s locally.
- **`.github/pull_request_template.md` untouched**, by instruction: it
  dies with T3b, after the last pull request has merged.
- **`docs/decisions.md` and `docs/orchestration/board.md` untouched**,
  by instruction — `herd-merger` writes both. No `D-NEXT` entry here.
- **`workflow.md` and `.claude/agents/` untouched**: T2 owns them. Two
  of them read the old names; see *Risks and findings*.
- **The four Rust gates were NOT RUN**, and are not reported as passed.
  Reason: the diff is two files under `docs/orchestration/templates/`
  and `scripts/check-docs.sh`. No `.rs`, no `Cargo.*`, no `flake.*`, no
  `.nix`, no `scripts/test.sh`, no `scripts/report-skips.sh` — nothing
  the Rust toolchain reads. `scripts/check-docs.sh` is outside `docs/`,
  so by the letter of the contract all five gates apply; the brief
  allows reporting them not run with the reason, and this is the reason.
  CI's `check` job runs them.

## Gates

`./scripts/check-docs.sh` — verbatim, exit 0:

```text
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```

The `note:` line is pre-existing on `origin/main` and is a note, not a
failure (D148: gaps stay). `./scripts/check-orchestration.sh` — exit 0,
last line verbatim, 0 `FAIL` lines and 146 `ok` lines:

```text
check-orchestration: 146 passed, 0 failed
```

**Skips:** no skip. Neither docs gate has a skip concept, and no
`HERD-SKIPPED` line exists in either output. The suite that produces
skips (`scripts/test.sh`) was not run — see *Not done*.

### The new case, red then green

Driven by two stub handoffs in a scratch copy of the corpus, run under
**gawk 5.4.1** (this host) and under **mawk 1.3.4** (the CI runner's
awk, from the nix store, symlinked as `awk` on `PATH`). Byte-identical
findings under both; the gawk half is quoted.

```text
=== no landing yet: 21 pre-cutover merge handoffs, none read
    rc=0
    docs ok
=== RED: a landing with no record
    rc=1
    a landing on or after 2026-10-28 must carry the filled landing record of docs/orchestration/templates/landing.md:
        docs/orchestration/history/2026-10-28-merge-batch-6.md: section "## What changed and why", section "## Branches in this batch", section "## Evidence", section "## Left out", section "## Decisions allocated", section "## Board rows", section "## CI run", a filled **Batch:** line, a filled **Gate:** line naming the sha it ran on, a filled **Skips:** line, the skips named (HERD-SKIPPED lines, or "no skip"), a filled **CI run:** line with the run id
    docs check failed
=== GREEN: the record, filled
    rc=0
    docs ok
=== RED: Gate line still <sha>
    rc=1
        ...: a filled **Gate:** line naming the sha it ran on
=== RED: CI run section dropped
    rc=1
        ...: section "## CI run"
=== RED: a skip count with no list (D27)
    rc=1
        ...: the skips named (HERD-SKIPPED lines, or "no skip")
=== RED: CI run field still <id>
    rc=1
        ...: a filled **CI run:** line with the run id
=== RED: the template pasted verbatim, nothing filled in
    rc=1
        ...: a filled **Batch:** line, a filled **Gate:** line naming
        the sha it ran on, a filled **Skips:** line, a filled
        **CI run:** line with the run id
```

Six red paths and one green. The `...` lines are elided only in the
indented file-name prefix; the full output of both awk runs is in my
scratchpad, under `gates/`:

```text
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/method-landing-record/gates/redgreen-final.txt
```

### Timing, against the `docs` job's 5-minute cap

`ci.yml:62-69`: the `docs` job is `timeout-minutes: 5` and runs
`check-docs.sh` then `check-orchestration.sh`.

| measurement | before (`bc83717`) | after |
|---|---|---|
| `check-docs.sh`, 40 runs, 3 rounds alternating | 270.2 / 271.6 / 277.8 ms per run | 269.4 / 269.6 / 271.2 ms per run |
| `check-orchestration.sh` whole, 4 rounds alternating | 12.15 / 12.06 / 11.98 / 12.02 s | 12.80 / 12.92 / 12.90 / 12.92 s (with the `basename` forks) |
| `check-orchestration.sh` whole, final shape | 12.05 s (mean of 3) | 12.09 s |

Read it in that order, because the middle row is the one that was
redesigned. The first shape of the rule cost **+0.84 s** over the suite
— 21 `basename` forks per run, 32 runs. Replacing them with
`${f##*/}` put the cost **under the measurement noise**: per-run cost
after is 269-271 ms against 270-278 ms before, i.e. **< 1 ms**, and the
whole script is 12.09 s against 12.05 s. Both before and after are
**146 passed, 0 failed**, so the rule adds no case to that suite and
costs it nothing.

Scaling, since a landing arrives per batch and the corpus only grows
(same 40-run method, alternating):

| landings in the corpus | before | after | delta per run |
|---|---|---|---|
| 0 (today) | 270.2 ms | 269.4 ms | < 1 ms |
| 1 | 270.3 ms | 274.4 ms | +4 ms (one `awk`) |
| 50 (≈ a year of batches) | 374.4 ms | 411.4 ms | +37 ms |

At 50 landings the rule costs +1.2 s over the 32-run suite, and the
already-existing 79-column rule costs +3.3 s over the same 50 files, so
this rule is never the thing that takes the job to its cap. Local
12.09 s against a 300 s cap; the CI runner ran 5 m 12 s when the local
figure was ~147 s, so ≈ 2.1× slower, which puts this suite near 25 s on
CI. Measured on `servernixos`, 12 cores, load1 2.0-4.0 (two other
agents live), which inflates both columns equally.

## Risks and findings

1. **Where the brief is wrong — nothing reads `**PR:**` as a field, but
   two files read the old *names*.** I grepped the whole tree. No
   script, template or agent file matches on the field name: the only
   hits outside `history/` are `.claude/agents/herd-merger.md:47`, which
   names `.github/pull_request_template.md` and its *Branches in this
   batch* section, and `docs/orchestration/workflow.md:52`, which says a
   PR is filled in from that template. Both belong to T2/T3b, and I did
   not touch them, as instructed. Consequence while they are unchanged:
   `herd-merger.md` points at a template that T3b deletes. 68 handoffs
   under `history/` carry `- **PR:**`; they are history and must not be
   rewritten.
2. **The record is checked in its copy, not in the commit message.** The
   merge commit message is not in the tree, and `check-orchestration.sh`
   copies the corpus without `.git`, so no docs-gate rule can ever read
   `git log`. What my case enforces is that the merger's handoff carries
   a filled record. A merger who fills the handoff and pushes a one-line
   merge commit passes the gate and leaves the real record empty. The
   only mechanism that would close that gap is the `pre-push` hook of T5
   (compare `git log -1 --format=%B` against the handoff), and it is not
   in my scope. Say it in `workflow.md` or lose it.
3. **The cutover date is a constant in the script**
   (`landing_from=2026-10-28`). It has to be: applying the rule to the
   21 merge handoffs written under the pull-request method would make
   `check-docs.sh` red on `origin/main` for files nobody may rewrite. If
   the method lands on a later date, that constant moves with it — one
   line, and the rule is a no-op until a landing is dated on or after
   it. If T6 slips past midnight, the first landing is still covered,
   because the constant is a floor.
4. **`<` is how "unfilled" is detected.** A `**Gate:**`, `**Batch:**`,
   `**Skips:**` or `**CI run:**` line containing `<` is treated as still
   holding a placeholder. A merger who writes a real `<` in one of those
   four lines (an email address in angle brackets, say) gets a false
   red, with a message that names the field. Cheap, portable, and the
   failure is loud and self-explaining rather than silent.
5. **mawk was tested explicitly.** The first shape used an awk array
   with `delete`; I rewrote it as a string accumulator so the program
   uses no construct beyond POSIX awk, then ran the whole red/green set
   under mawk 1.3.4 as well as gawk 5.4.1. Byte-identical. This matters
   because the comment at `check-docs.sh:31-48` records that awk
   flavours already bit this script once.
6. **I did not reach across into T1, T2, T3 or T5.** No collision: `git
   status` on my branch is three files, two of them new or mine alone.

## Decisions needed

**Question:** should the landing record's sections keep the pull-request
template's exact names (*To record*, *Handoff*) as the plan's §4 says,
or the adapted names I used (*Decisions allocated*, *Board rows*) as the
brief says? **Recommendation:** keep the adapted names — "Handoff" names
the wrong artefact inside a merge commit, and the section's content is
board rows. **Fallback:** two `sed` lines in `landing.md` and two words
in the `check-docs.sh` heading list; no other file depends on them.

## Resume point

Nothing blocks. Next, in order:

1. `herd-merger` adds the board row for this branch and allocates its
   `ORCH-nnn`; T2 is the one that must rewrite
   `.claude/agents/herd-merger.md:47` and
   `docs/orchestration/workflow.md:52` to name
   `docs/orchestration/templates/landing.md` instead of
   `.github/pull_request_template.md` — finding 1.
2. After T1 and T5 have landed their sections of
   `check-orchestration.sh`, a new board row: add six
   `expect_red`/`expect_green` cases for the landing rule, built from
   `.../scratchpad/method-landing-record/fix-none.md` and
   `.../scratchpad/method-landing-record/fix-full.md` — finding 1 under
   *Not done*.
3. `workflow.md` says plainly that the handoff copy is checked and the
   commit message is not — finding 2.

## Processes

PIDs started and ended: none that outlived a command. Every run was a
foreground `./scripts/check-docs.sh` or `./scripts/check-orchestration.sh`
in my own worktree, with `TMPDIR` pointed at my scratchpad so the
harness's `mktemp -d` stayed out of `/tmp`. No `cargo`, no build, no
`target/` directory created, so none to delete. Nothing killed.
