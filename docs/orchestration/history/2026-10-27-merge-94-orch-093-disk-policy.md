# merge 94 orch-093-disk-policy — handoff 2026-10-27

- **Item:** ORCH-093
- **Agent:** herd-merger
- **Branch / commit:** `orch-093-disk-policy` / the merge of this
  integration, based on origin/main `a789153` (PR #93)
- **PR:** #94

## Done

`origin/main` merged into the branch. **No conflict**: the branch adds
two files and touches nothing `origin/main` touched. The product code is
untouched — `git diff origin/main HEAD` before this integrator's own
commits was `docs/disk-policy.md` and one handoff, 818 lines added, zero
removed.

### The rows, and where each came from

The document's §7 drafts its own board rows, deliberately, because
`herd-merger` owns `docs/orchestration/board.md` and the planner has no
write on it. **Twelve rows were brought across and one was allocated
here.** Each is placed in the section its state names, which is what the
board does everywhere else.

| Row | Section | State | Source |
|---|---|---|---|
| ORCH-093 | Owed | done | §7, this PR |
| ORCH-095 | **Waiting** | waiting | §7 plus §6 **decision B** |
| ORCH-096 | Owed | owed | §7 |
| ORCH-097 | Open | open | §7 |
| ORCH-098 | **Waiting** | waiting | §7 plus §6 **decision C** |
| ORCH-099 | Owed | owed | §7 |
| ORCH-100 | Owed | owed | §7 |
| ORCH-101 | Owed | owed | §7 |
| ORCH-102 | Owed | owed | §7 |
| ORCH-103 | **Waiting** | waiting | §7, §6 **relay D** |
| ORCH-104 | Open | open | §7 |
| ORCH-105 | **Waiting** | waiting | §6 **decision A**, which §7 gave no id — **allocated by this integration** |

**ORCH-095, ORCH-098 and ORCH-103 are in `## Waiting` and not in
`## Owed`**, because that is the section Alexa reads and all three are
stopped on her. Their cells are written in the Waiting table's own
columns — Question, Recommendation, Fallback — out of §6's text, with
the work each unblocks named in the recommendation so nothing of §7's
row is lost. **None of the three is re-opened as fresh**; each says it
is the decision the document already put to her.

**ORCH-105 is the one id this merger invented.** §6's decision A — the
shipped seven-day output floor against a workload that cannot keep it —
had no row anywhere. It is now one, with §6's recommendation ((i) and
(iii) together) and §6's fallback ((ii) at three days) as written, and
**it is not decided here**: it changes a shipped default and is
outward-facing.

**ORCH-094 was deliberately not brought across.** PR #97
(`orch-094-id-highwater`, open) had already written an ORCH-094 row for
the same item — it *is* the implementation §7 asked for — and
`check-docs.sh` refuses an id twice. **PR #97's row is the one that
stands**; §7's draft is marked in the document as not brought onto the
board.

### Two corrections made at integration, declared as such

Neither was reviewed with the rest of this branch.

1. **`docs/disk-policy.md` §3.6 and §7 proposed the wrong home for the
   job-id high-water mark.** §7 said `IndexFile` — `index/state.json`.
   PR #97 put it in **`store.json`** beside `state_root_id`, and the
   reason is evidence and not taste: `index/state.json` is *derived*,
   and every path in `usable_index` that doubts it throws it away and
   re-reads the records, which is right for a summary the directories
   can rebuild and fatal for a number nothing on disk can reconstruct
   once those directories are gone. §3.6 now carries that paragraph,
   §7's row carries the outcome, and the file-ownership list says
   `StoreFile` instead of `IndexFile`. **The planner's diagnosis is
   untouched and was right in full; only the placement is overturned.**
   It was corrected here, rather than left for PR #97, so that `main`
   is not self-contradicting at any commit — the same call PR #91's
   integration made.
2. **The board's `## Next` item 4** said "No row waits on Alexa any
   more" and described ORCH-093 as "held outside this board and with no
   row on it yet". Both were made false by this merge. Item 4 now names
   the four rows that wait and says **A first**, because it is the only
   one of them that changes what is shipped.

### The placement of `docs/disk-policy.md` was checked, and the check passed

§5 says `docs/remediation/` is "a closed dossier about something else
and nothing here belongs in it", and places the document at the top
level beside `docs/runner-confinement.md`. **The placement is right and
nothing disputes it**: `scripts/check-docs.sh` applies its prose rules
to `docs/*.md`, so the file is checked where it sits, and no index names
it or claims it. **The word "closed" is not accurate, though, and the
PR body says so:** `docs/remediation/README.md` reserves numbers for
pieces not yet written (`01`, `02`) and PR #92 is adding `12` to it
right now. What is true is the part the placement rests on — the dossier
is the audit required by the 2026-10-24 mandate, and this document is a
planner's derivation from the consumer's figures, not an audit piece.
The sentence was left as its author wrote it.

### `docs/decisions.md`

**Untouched, and no `D` number was allocated, deliberately.** This
document is an analysis, not a decision; its decisions are the three
that now wait on Alexa (ORCH-105, ORCH-095, ORCH-098). The `D170` gap
stays a gap (D148).

## Not done, and why

- **`docs/status.md` was not touched, and it should have been.** It is
  at its hard limit of 90 lines, and the finding that belongs in it —
  the shipped seven-day output floor cannot be kept at the first
  consumer's workload on their disk, at any budget — does not fit
  without cutting lines this integration did not write and was not
  asked to judge. The board carries the finding and all four waiting
  rows. **The next rewrite of `status.md` should carry it**, in the
  disk bullet that already names their volume.
- **No row was written for ORCH-092**, which is in flight with no board
  row. It is not this branch's.
- **Nothing on the consumer's side**: ORCH-103's listing was not run,
  asked for, or worked around (D161, invariant 9).

## Gates

**Prose only.** `git diff origin/main HEAD` touches `docs/` and nothing
else — no Rust, no Cargo, no `ops/`, no `scripts/`. The four Rust gates
were **NOT RUN** locally; CI runs them on the PR.

```text
$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```

That note is pre-existing and is about `decisions.md`, which this branch
does not touch. **No skip**, because nothing was run that could skip.

**On the host's suite, for the record and not as this branch's
evidence:** `scripts/test.sh` is red on this workstation, and at the
integration of PR #93 an hour earlier it was red in **five** targets
with **21 failures**, nineteen of them timeouts at
`real_herd/mod.rs:649`/`:789` rather than mismatched values — more than
the two ORCH-069 describes. That was proved not to be the branch there
by the Rust tree being byte-identical to `origin/main`; the same holds
here, where no Rust file is touched at all.

## Risks and findings

1. **PR #97 must land for §3.6's added paragraph to be true of `main`.**
   It describes `ids_minted` in `store.json` as implemented, and it is —
   on `orch-094-id-highwater`, not yet on `main`. The paragraph names
   the PR, so a reader can tell. **If PR #97 is ever closed unmerged,
   that paragraph and §7's ORCH-094 row must be taken back out**, and
   ORCH-094 then needs a board row of its own, which this integration
   deliberately did not write.
2. **Four rows now wait on Alexa at once**, against the board's "one at
   a time". They arrived as one document and could not be split without
   inventing an order the document did not give; the `## Next` entry
   says **A first** and why.
3. **ORCH-095 and ORCH-098 sit in `## Waiting` while also being work
   items.** When each is answered, the row has to move or be split: the
   decision is then done and the task is not. Whoever answers them
   should expect to re-file them under `## Owed`.
4. **Nothing in this document has been measured by this project.** Its
   own §4 says so: H1, H2 and H3 need a read on the consumer's host that
   invariant 9 forbids us, and H6 needs a fixture measurement nobody has
   run (ORCH-104). The rows carry that; the figures should not be quoted
   as Herd's measurements.

## Decisions needed

Four, all now on the board and none of them this project's: **ORCH-105**
(A, the seven-day floor), **ORCH-095** (B, `--volume-free-floor`),
**ORCH-098** (C, the 90/14 class map) and **ORCH-103** (D, the relay).
One at a time, A first.

## Resume point

PR #97 (`orch-094-id-highwater`), whose ORCH-094 row is already written
and whose CI had not started when its author handed back — verify it is
**green** and not merely not red before merging. Then PR #92, PR #95 and
PR #96, each of which will conflict with `docs/orchestration/board.md`
as it now stands.

## Processes

None started and none ended by this integration: it ran no build and no
test. No process of another agent was signalled; no `pkill`, no
`killall`.
<own `target/` deleted once the branch was pushed — this integration
built nothing, so there is no `target/` in its worktree; the one from
PR #93's integration, 8.3 GB, was deleted as soon as that branch was
pushed. The reason is in workflow.md "Concurrency and host rules">
