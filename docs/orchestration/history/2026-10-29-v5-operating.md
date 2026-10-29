# v5-operating — handoff 2026-10-29

- **Item:** ORCH-NEXT (proposed below; the integrator allocates the id)
- **Agent:** herd-step
- **Branch / commit:** `v5-operating` / `e7ac24a`, based on origin/main
  `cd0f1b7e30aa7401a11dc8ced14d41726c0bc18e`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

`docs/operating.md`, 805 lines, new, the only file this branch touches.
It is addressed to one reader: the person who runs the hub and a runner
in docker compose on a machine nobody here will ever see.
`deploy/release-notice.md` was read first and is the voice it follows —
every claim read out of the code it describes or measured, and whatever
nobody has checked named as unchecked in the sentence that makes it.

Twelve sections, each answering one of the brief's items:

1. **Four words, and the two you use differently.** hub, runner,
   requester, job, and the mapping — their "client" is our runner,
   their "agent" is our requester — said once so they can translate.
2. **Reading what the state root holds.** `herd-hub report` and
   `herd-hub retention-preview`: what each reads, what neither touches,
   and why neither can stall a serving hub (no store lock, no `flock` on
   `hub.lock`, no output or log byte, nothing created). The snapshot
   caveat in the report's own words.
3. **The recommended way: a disposable container, read-only, no
   network.** The consumer's own method, credited to them and marked as
   theirs. Given with the hub image they already have as the base, since
   `--network none` means nothing can be pulled. Five properties, read
   out of the code: the serving hub is untouched, the mount is read-only,
   no port is ever bound, it works when their serving image predates the
   subcommand (measured: `report` is absent from `v0.3.7` and present in
   `v0.4.0`), and it works when a torn record would stop a hub starting.
4. **The retention preview and its two refusals.** Four required spans
   with no default, `0` meaning never, and the two threshold-free
   refusals of D197 with the forward-clock argument stated. Non-zero
   exit on a refusal and why.
5. **The status document and the text screen: not yet shipped.** E-35,
   `planned` at M13 in `docs/coverage.md` at this base sha; no `status`
   subcommand exists in `herdctl`. The section says a sibling task is
   building them and is to be rewritten against what ships. A table of
   what answers the same six questions today, all of it shipped.
6. **The two budgets and the two retention spans.** A table of all four
   with where each is set, its default and the file the default lives in:
   `--output-budget` (hub, 200 GB decimal), `--job-disk-budget` (runner,
   20 GiB), `--output-floor` (hub, 7 days), and record retention —
   **named as enforced by no shipped build**, with D201's 90/14 map as
   decided and unbuilt. `--job-output-cap` named beside them so it is not
   read as a budget.
7. **What happens at the floor.** The floor is a floor and not a
   lifetime; the sweeper fires on an arrival and never on a clock; the
   four-key order; the budget wins and the hub says so on the day, with
   both of the hub's real sentences quoted and their figures marked
   illustrative.
8. **Changing one, and what it costs.** Both hub figures are read at
   startup and opening the root sweeps, so lowering the budget evicts on
   that start — read the root first. The hub's bounded five-second stop
   (D184) against compose's 20 s grace. The runner figures as a unit
   restart. And *What is changing, and is not shipped*: retention's basis
   moving from a calendar to free space, explicitly marked as newer than
   this page's evidence — no numbered decision at this sha, no code, no
   release — with shipped, decided-and-unbuilt and changing kept apart.
9. **Approving, revoking and updating a runner.** The three verbs as
   they really are: `approve`, **`remove`** (not "revoke"), `list`; the
   token to stdout alone and once; re-approving as the rotation; the
   empty roster refusing everything; a removal leaving a running job
   alone and `cancel` being the thing that kills one; the switch as the
   answer when the machine is wanted back. **No self-update in any
   shipped build** (M15d planned), so updating is binary plus restart,
   with the patch-versus-protocol distinction and the v0.4.0 exception.
10. **Reading a refusal.** The three endings kept apart: a job that
    waits (nobody offers the labels *today*, suspended capability,
    stickiness, a queued reading), `impossible` as deliberately narrow —
    what no fleet can fix — and `refused_as_full` as its opposite,
    nothing failed and nothing lost. All three wear 125; the
    discriminator is `cause` in `--verdict-to`, never the number and
    never the prose.
11. **Reading a tombstone answer.** The manifest outliving the bytes,
    the collection's one line then the per-file names, and the field that
    makes "not yet" different from "gone" — with the reason it is
    deliberate: silence is what makes somebody conclude Herd lost the
    evidence.
12. **When a version refusal names two figures.** Written as the thing
    read at the worst moment: the three numbers and today's values
    (2, 1, 1), **no migration path anywhere**, both refusal sentences
    verbatim, the refusal cutting both ways so it is the rollback too,
    and then the instruction in order — take the reading off the old root
    **first** with the old release's binary, then move the root aside
    without converting or deleting it, expect every runner to be refused
    once by name (the foreign-state-root sentence quoted), re-approve.
    Closed with what it means today: nothing shipped moves the last two,
    the next ordinary release is held at record 1 and layout 1 (D199),
    and v1.0.0 is the release that moves them.

Then the brief's two extras: **the shared requester token** as one
stated limitation with no fix designed (any requester holding it may ask
for `merger`, plus the hub's own no-token warning verbatim); **two known
defects with their mitigations**, each a section — the torn `job.json`
refusing the whole root before the port is bound, with `report`'s
contrary behaviour as the mitigation and the order of operations as the
practical advice, and v0.4.0's startup reaper deleting the spools its own
sweep re-adopts, with a per-tag affected table (below) and what the fix
changes. A closing *What nobody has checked* and a pointer table.

**One figure measured rather than relayed.** The brief says "say which
versions are affected". Measured here against the tags in the repository
rather than inferred from the decision record:

```
$ for t in v0.3.6 v0.3.7 v0.4.0; do printf '%s: ' "$t"; \
    git merge-base --is-ancestor 0c14997 "$t" && echo "reaper clears \
    outputs/" || echo "does NOT"; done
v0.3.6: does NOT
v0.3.7: does NOT
v0.4.0: reaper clears outputs/

$ for t in v0.3.6 v0.3.7 v0.4.0; do printf '%s pick_up_collections: '; \
    git grep -c pick_up_collections "$t" -- herd-runner/src/lib.rs; done
v0.3.6 pick_up_collections: 0
v0.3.7 pick_up_collections: 0
v0.4.0 pick_up_collections: 7
```

So **v0.4.0 is the only released version affected**, and the document
says that rather than "0.4.0 and above" or "recent releases". The fix
`339d1ba` is an ancestor of `origin/main` and is in no release.

```
$ git merge-base --is-ancestor 339d1ba origin/main && echo on main
on main
```

## Not done, and why

- **`docs/status.md` and `docs/orchestration/board.md` are untouched**,
  as the brief requires: they are the integrator's. The board row is
  proposed under *Decisions needed* and not written.
- **No file outside `docs/` is touched**, and `docs/operating.md` is the
  only file in it that changed. The stale `herd-runner --help` text
  found on the way (below) is a finding, not an edit.
- **The status-document section is marked *not yet shipped* rather than
  written against the sibling task's delivery.** That branch is not on
  `origin` and not visible from this worktree, so writing against it
  would have been describing something this agent could not read. The
  section is one heading plus a table and is cheap to replace; whoever
  lands M13's half should replace it.
- **No `cargo` command was run at all**, as the brief requires. No
  compile, no suite, no bench, so `./scripts/bench-preflight.sh` was not
  reached either.
- **Nothing of the consumer's was read.** No path on their instance, no
  volume, no container. Every figure about their machines in the document
  comes from `docs/disk-policy.md` §1.2 or `docs/status.md` and is marked
  as a relayed, unverified report where it appears.

## Gates

Prose only — `docs/operating.md` and this handoff are the whole diff — so
**the four Rust gates were NOT RUN locally**. What covers them is the
batch's own `./scripts/gate.sh` run at integration, which is
`herd-merger`'s and not this agent's. A gate that did not run is reported
as not run, never as passed.

Run before this file was committed, on the tree carrying
`docs/operating.md` at `e7ac24a`:

```
$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```

The same command was run on the untouched base tree first, to establish
that the `D170` note is the baseline and not something this branch
introduced:

```
$ ./scripts/check-docs.sh     # base tree, before any edit
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```

Width was additionally audited by hand with the script's own awk, because
the brief names it as the control that has gone red three times: every
line in `docs/operating.md` over 79 columns is a table row, which that
check exempts.

**The last run**, on the committed tree including this handoff, is
`./scripts/check-docs.sh` in this agent's own worktree:

```
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/wt-v5-operating
```

Its output goes to this agent's report and to
`<scratchpad>/wt-v5-operating-checkdocs-final.txt`, not into the tree it
has just gated.

**Skips:** no skip. Nothing in this task runs an assertion, so there is
no `HERD-SKIPPED` line to copy and none is claimed as a pass.

## Risks and findings

1. **`herd-runner --help` is wrong about `--job-output-cap`, and it is
   the text an operator reads.** `herd-runner/src/main.rs` (the doc
   comment on `job_output_cap`) says "32 MiB by default, which is also
   the most this can be … A higher value is refused at startup with both
   figures". The constant is `DEFAULT_OUTPUT_CAP = 1 << 30`, 1 GiB
   (`herd-runner/src/confine.rs:82`), whose own documentation says at
   length that the flag may now *raise* it because there is somewhere
   above a frame to go. The two disagree, `--help` loses, and the help
   text is what reaches the operator. `docs/operating.md` states the
   constant and names the help text as a known documentation defect
   rather than repeating either figure silently. **Outside this brief's
   paths — it is a code change — so it is a finding and not an edit.**
   Worth a small board row.
2. **Retention's new basis is documented as unrecorded, and that will
   read as wrong the moment it is recorded.** The move from a calendar to
   free space is in the brief and in nothing else this agent can read: it
   is not a numbered entry in `docs/decisions.md` at `cd0f1b7`, where
   D201 still states the 90/14 calendar map, and no board row mentions
   free space. The document says exactly that, with the sha-anchored
   claim. If the decision lands as a `D` number in the same batch, that
   paragraph needs one sentence changed to cite it. Flagged so the
   integrator does not have to find it.
3. **The disposable-container recipe is written from the method's
   properties, not from the consumer's transcript.** This agent never saw
   the command they ran. What is verified is that it *can* work and is
   safe: `report` takes no lock, writes nothing, creates nothing and
   never reaches the port-binding path; the hub image's entrypoint is
   `/bin/herd-hub` and its user is uid 1000, so the override and the
   read-only mount are both sound. The base image in the example is their
   own loaded hub image, chosen because `--network none` forbids a pull.
   If their real command differs — a different base image, a different
   volume name — the recipe is still correct but is not a transcript of
   theirs, and the section says the method is theirs rather than that the
   command is.
4. **The volume name `herd_herd-state` is taken from
   `deploy/docker-compose.yml`** (`name: herd` plus volume `herd-state`)
   and matches what `docs/disk-policy.md` §1.2 R1/R5 relays. It is still
   the default rather than a reading of their host, and the document says
   "if you took `deploy/docker-compose.yml` as it is".
5. **Line numbers in `docs/disk-policy.md` §1.1 have drifted** for the
   two output constants: it cites `herd-hub/src/job.rs:994` and `:998`,
   and they are now `:1078` and `:1082`. `docs/operating.md` deliberately
   cites files without line numbers for this reason. Not fixed — it is
   another document and outside the brief.
6. **The document is 805 lines**, between `docs/joining.md` (984) and
   `docs/handover.md` (789), and it is not covered by any length rule in
   `check-docs.sh`. If the integrator wants it shorter, the two
   candidates are the pointer table at the end and the per-section
   rationale in *What happens at the floor*, both of which can go without
   losing an instruction.

## Decisions needed

None for Alexa. One **board row proposed**, not written — the brief
reserves `board.md` to the integrator:

| Field | Proposed content |
|---|---|
| Type | docs |
| Question / item | **`docs/operating.md`: the document the consumer's operator did not have.** Reading a state root (`report`, `retention-preview`, the consumer's disposable-container method made the recommended way), the two budgets and the two retention spans, approve/revoke/update a runner, reading a refusal and a tombstone, the version refusal's order of operations, the shared requester token as a stated limitation, and the two defects found 2026-10-29 with their mitigations |
| Origin | V5 mandate, 2026-10-29 |
| Owner | herd-step |
| State | done |
| Next action | Two follow-ups it leaves: the status-document section is marked *not yet shipped* and must be rewritten when M13's half lands; and `herd-runner --help` still prints the old 32 MiB sentence for `--job-output-cap` against a 1 GiB constant, which is a code fix and wants a row of its own |
| Dependency | ORCH-106, ORCH-161 (the torn-record defect it documents), ORCH-164 (the v0.4.0 spool defect), D199 (the version refusal), D201 and D202 (the spans) |
| Evidence | `docs/operating.md`; `history/2026-10-29-v5-operating.md`; branch `v5-operating` |

## Resume point

Nothing is in flight. For whoever continues:

```sh
git fetch origin && git log --oneline origin/v5-operating -1
# expected: the handoff commit on top of e7ac24a
```

Then either take the branch into the next batch's integration, or pick up
one of the two follow-ups named in the row above. The sibling task
building M13's status document and text screen owns the one section of
this file that is a placeholder.

## Processes

None. No background command, no server, no compile, no test binary; no
PID was started and so none was killed, and nothing was killed by name.

No `target/` was created, because no `cargo` command was run; there is
therefore nothing to delete and `CARGO_TARGET_DIR` was never set.

This agent's own worktree holds the branch:

```
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/wt-v5-operating
```

It is left in place for the integrator and removed with
`git worktree remove` once the branch has landed.
