# reaper-sweep — handoff 2026-10-29

- **Item:** ORCH-164
- **Agent:** herd-step
- **Branch / commit:** `reaper-sweep` / see the agent's report, based on
  `collect-gap` `7b3bce7`, itself based on `origin/main` `10ff31d`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

One defect, in the code only. No version number moved: `PROTOCOL_VERSION`
stays 2, `RECORD_VERSION` 1, `LAYOUT_VERSION` 1, and nothing on the wire
or on a record changed. Nothing of the consumer's was read (D161).

### The defect, confirmed on our own source

`run_until` runs two things over the same directory. At
`herd-runner/src/lib.rs:2634` it calls `Confined::prepare`, which calls
`reclaim_a_past_life` (`confine.rs:504`), which walked **every** entry
under `outputs/` with no guard and `reclaim_directory`d it —
`fs::remove_dir_all`. Twenty-eight lines later, at `lib.rs:2662`, it calls
`Session::pick_up_collections` → `sweep::pick_up`, which exists precisely
so a spool a previous life left **is** offered to the hub again. The
second read what the first had just emptied.

The reaper's own sentence was the proof that the premise was false:

> no runner can offer a spool it did not make, so these bytes could never
> have reached the hub

The runner twenty-eight lines down is the runner that does.

Dates, measured here: the reaper's `outputs/` arm is `0c14997`
(2026-10-25), the sweep is `b250358` (2026-10-27), both inside
`v0.3.7..v0.4.0`. So the sweep arrived dead in the release that shipped
it, and the arm it collided with was two days older.

### The choice: the reaper cedes `outputs/` to the sweep

The brief offered *order* and *a guard*. This is the guard taken to its
limit — the guarded set is the whole directory — which is why it costs no
shared notion of "recognisable" and nothing can drift out of agreement.

**Why not order** (sweep first, reaper removes the rest). What the sweep
does *not* take is exactly what it decided not to touch for safety: a
spool whose walk hit `MOST_ENTRIES_SWEPT` or an unreadable subdirectory
(`sweep.rs`, the `whole` flag), one already set aside `.unclaimed` or
`.foreign-state-root`, a name that is not UTF-8, a symlink. Letting the
reaper delete the remainder would delete unacknowledged bytes on a busy
disk and would delete bytes the sweep has promised an operator are theirs
to remove. Order turns the sweep's carefulness into a delete list.

**Why not a narrow guard** (reaper skips what `parse_name` accepts). It
would still delete `.unclaimed` and `.foreign-state-root`, which is the
one thing `the_hub_has_said_what_it_is_owed` promises it will not do, and
`a_spool_already_set_aside_is_counted_and_never_offered_again` asserts.
Everything else it would then delete — odd names, files, symlinks — is
something this runner never wrote and that never grows. All cost, no gain,
plus the two-places-must-agree duplication the brief named.

**Why ceding is right rather than merely safe.** `DataRoot::outputs_dir`
(`root.rs:199-205`) already says in the tree why `outputs/` sits outside
`jobs/`: *the startup reaper clears a workspace, and a collection the hub
has not acknowledged is the only copy of a result (D19)*. The v0.4.0 arm
contradicted a comment that was already there. And the reaper's condition
for removing something — *nothing can offer it* — is now never satisfied
under `outputs/`: every byte there gets a disposition from the sweep.

The false sentence is deleted, not reworded. In its place the body of
`reclaim_a_past_life` carries why the directory is not the reaper's, what
it cost when it was, and where its dispositions now come from; the
function's doc comment says "processes, workspaces and cgroups, and
nothing else"; `confine.rs`'s call site no longer lists output spools.

### What now happens to a spool, in all four cases

1. **The hub asks for it.** It resumes on the ordinary path, is
   acknowledged, and `Session::files_acknowledged` removes it — the one
   path that ever removed a spool, unchanged.
2. **The hub already holds every byte.** One chunk is both resumption and
   question; the store answers "already here and whole", the collection is
   acknowledged on the spot and removed the same way. Cost: one frame.
3. **The hub is silent** — the record is gone, the attempt was superseded,
   the collection was evicted there, or this machine is not the one the
   record names. `the_hub_has_said_what_it_is_owed` renames it
   `.unclaimed`, names it with its figure, and never offers it again.
4. **It was produced for a hub state root that is gone.** Renamed
   `.foreign-state-root`, for the reason in `sweep.rs`: a job id means
   something only inside one hub state.

**No path deletes an unacknowledged spool.** Removal happens only on a
sentence from the hub (1 and 2). **And acknowledged spools are not kept
for ever:** if the `remove_dir_all` in `files_acknowledged` fails — the
D158 case, a directory this uid cannot write — the next start sweeps the
spool up again, offers it, is acknowledged on the spot by case 2, and
retries the removal. Self-healing, every start, with the figure named.

**What bounds the keeping in cases 3 and 4:** visibility and a fixed set,
not a timer. A set-aside spool is terminal — nothing writes into it again,
it is never offered again, and it cannot multiply — and every start counts
and names what is in there, with the sentence *removing them is yours to
do*. That is the shipped asymmetry and it is deliberate: acknowledgement
is a sentence, silence is not, and a hub whose record scan fails is silent
in the same way to every runner on the fleet. **This fix does not give
those bytes an automatic end, and that is a gap worth a board row** — see
*Risks and findings*.

### The test, and the gap that let this ship

`the_reaper_does_not_delete_a_spool_this_sweep_would_have_resumed` is no
longer `#[ignore]`d. It was also rebuilt, and the reason is the whole
point of it: the version on `collect-gap` *re-implemented* the startup
order in the test body ("exactly what `Confined::prepare` does, in the
order it does it"). A stand-in for the startup path is what let two green
tests cover a broken composition. It now calls the runner's own entry
points and stands in for neither:

- `crate::confine::Confined::prepare(Confinement::with_data_root(…))` —
  the real `lib.rs:2634` call, which is what calls the reaper;
- `crate::Session::pick_up_collections(confined.root())` — the real
  `lib.rs:2662` call, on the data root that confinement opened;
- then `settle_against_state_root`, so the spool is shown reaching the
  hub's resumption path and not merely sitting on a disk.

It reads the reaper's words off `Confined::report()` — the lines a runner
really prints at startup — rather than off a return value only a test
sees. It asserts the spool exists, that its 29 bytes are byte-identical,
that no `reaped at startup:` line claims it, and that the offer line names
`j0000001561`. The negative assertion is scoped to that job id and to the
removed sentence, not to the report being empty, because this host's own
stray cgroups are a shared machine's business and no part of the claim.

In `reclaim::tests`,
`the_reaper_reclaims_an_output_spool_a_previous_life_left_behind`
asserted the deletion. It is replaced by
`the_reaper_leaves_an_output_spool_for_the_startup_sweep`, which asserts
the spool survives untouched, its bytes are still 4096, and the reaper
says nothing about it. **This is a changed expectation, not a test edited
to turn a gate green:** the expectation *was* the defect, the brief's
mandate is to remove the behaviour it asserted, and the replacement is a
stronger claim about the same code path. The deleted assertion is quoted
in the new test's doc comment so a reader can see what moved.

## Not done, and why

- **ORCH-163, ORCH-165, ORCH-166** — out of scope by the brief, so that a
  v0.4.1 can carry this alone. Untouched.
- **`docs/decisions.md`** — not written. The D211 entry's text is proposed
  below for `herd-merger`, who allocates the number. A `D-NEXT`
  placeholder was *not* left in the file: `scripts/check-docs.sh:111`
  treats one as a failure, so leaving it would have handed over a red
  docs gate.
- **`docs/orchestration/board.md`** — not written; the ORCH-164 row's text
  is proposed below, as the brief asked.
- **`docs/remediation/09-runner-self-limits.md:333`** still names the
  deleted test and still says the reaper sweeps `outputs/`. Left alone: it
  is a dated record of the 2026-10-25 piece of work, not a live
  specification, and history is not rewritten. Worth reading, though — its
  *"What is deliberately not done, and it is the better fix. H."* names
  re-adopting the spool as the better fix and says nobody had asked for
  it. Two days later somebody built it and nobody removed this arm.
- **`herd-hub/tests/spools_resumed.rs`** still drives a fake runner over a
  raw socket and still never calls `Confined::prepare`. Closing that at
  the end-to-end level was not in scope; the unit test above now covers
  the composition through the real entry points.
- **No new `#[ignore]`, no new skip.** The branch's skip list is the
  host's twelve, unchanged.

## Gates

Run bare, never nested in `nix develop` (that nests `TMPDIR` and panics
`confinement.rs:2529` on `SUN_LEN`). The four Rust gates inside it use
`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/reaper-sweep`, off the root
filesystem. `./scripts/bench-preflight.sh` was run before the first
compile and said *ok, safe to start* (336 GiB free, load 0.64 of a 9.00
ceiling, 22 GiB available).

Before this file was committed, on the code commit's tree:

```
test reclaim::tests::the_reaper_leaves_an_output_spool_for_the_startup_sweep ... ok
test sweep::tests::the_reaper_does_not_delete_a_spool_this_sweep_would_have_resumed ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 184 filtered out
```

```
cargo fmt --all --check: clean
cargo clippy --workspace --all-targets -- -D warnings: Finished, no warnings
```

**Red before, on the same test.** With `herd-runner/src/reclaim.rs` and
`confine.rs` restored from `collect-gap` and the new test body in place:

```
panicked at herd-runner/src/sweep.rs:711:9:
the reaper removed <root>/outputs/j0000001561-2, so the sweep that exists
to re-adopt it reads an empty directory. The reaper said: ["reaped at
startup: reclaimed 29 bytes of output left in <root>/outputs/j0000001561-2
by a previous life: no runner can offer a spool it did not make, so these
bytes could never have reached the hub"]
test result: FAILED. 0 passed; 1 failed
```

That failure came through `Confined::prepare` and `Confined::report()`,
which is the evidence that the test exercises the real startup order and
not a copy of it.

**The last run**, on the committed tree including this file:
`./scripts/gate.sh --out <log>`, bare. `gate.sh` writes, under the
worktree's own `target/herd-gate/`, a receipt named for the sha it ran on,
the suite's output and the observed skip list; those three and the log are
kept outside the build tree, in this agent's scratchpad under
`receipts/`, since `target/` goes once the branch is pushed. The verbatim
output, the verdict line and the twelve skip names are in the agent's
report.

**Skips — the host's twelve, each a `HERD-SKIPPED` line from
`scripts/test.sh`, none of them a pass (D27, invariant 5), and none of
them this branch's.** They are the pre-existing cgroup-delegation skips of
a host with no delegated cgroup v2 subtree. The thirteenth entry of the
`collect-gap` run — the `ignored` reproduction — is gone from this
branch's list because the test now runs, and passes. The names are in the
agent's report.

## Risks and findings

- **The disk half of the reaper's concern is now carried by naming, not by
  deleting, and nothing ends it.** A `.unclaimed` or `.foreign-state-root`
  spool is kept until an operator removes it. That was already true in
  v0.4.0 for any spool the sweep set aside *after* a connection, and this
  fix makes it true for the whole directory. It is correct — D19 forbids
  acting on silence — but it is not a bound in time, and the first
  consumer's 396 MiB is exactly the shape that would sit there if their
  hub turns out to have written the job off. **Proposed as a board row**
  below; deliberately not built here, because an automatic end for bytes
  the hub never spoke about is a policy decision, not a bug fix.
- **`reclaim_a_past_life` still removes everything under `jobs/`**
  unconditionally, which is right (a workspace is reconstructible, a spool
  is not) and is untouched here. Worth knowing that the asymmetry between
  the two directories is now the load-bearing part of the design.
- **Not verified, and it cannot be from here:** that the first consumer's
  396 MiB is a spool of the shape `{job}-{attempt}` with a readable tree
  under it. If it is not — a name the sweep cannot parse, a tree it cannot
  walk through — then after this fix it is *kept and named* rather than
  deleted, which is the safe outcome, but it is not *resumed*. Their
  runner's startup report names which, and that is theirs to read.
- **The upgrade advice changes.** With this fix a v0.4.1 runner resumes
  what v0.4.0 would have deleted. Until it lands, the hold on their
  upgrade is the right call.

## Decisions needed

1. **Proposed `docs/decisions.md` entry, for `herd-merger` to append with
   the next free number — measured as D211 against `origin/main`
   `10ff31d`, whose highest is D210.** Text:

   *Title:* `The startup reaper does not touch outputs/; the sweep owns it`
   *Body:* D180 gave the startup reaper `<data root>/outputs/` on the
   premise that no later runner could offer a spool it did not make. D180's
   own entry says the `outputs/` half is "the one irreversible half" and
   that "a revert simply stops the sweep". Two days later `b250358` built
   the runner that *can* offer such a spool, on the same startup, twenty-
   eight lines after the reaper, over the same directory. v0.4.0 shipped
   both: the sweep read what the reaper had emptied. The reaper now owns
   processes, workspaces and cgroups and nothing else; `outputs/` is the
   sweep's alone, which is what `DataRoot::outputs_dir` already said in
   the tree. Removal of a spool stays on the hub's sentence
   (`files_acknowledged`, D19); silence sets the bytes aside under a name
   and leaves the disk to a human. Rejected: ordering the two stages
   (deletes the bytes the sweep carefully declined to touch) and a narrow
   `parse_name` guard (deletes the `.unclaimed` set the sweep promises an
   operator). No version moved. The test now drives `Confined::prepare`
   and `Session::pick_up_collections` rather than re-implementing their
   order, because a stand-in for the startup path is how this shipped.
   *Recommendation:* append as written, numbered by the merger.
   *Fallback:* if a decision entry is judged too heavy for a bug fix, the
   commit message carries the same argument and the board row below
   carries the pointer.

2. **Proposed `docs/orchestration/board.md` row for ORCH-164**, for
   `herd-merger` to write (columns in the board's order: id, what, where
   it came from, who, state, next step, blocked on, evidence):

   `| ORCH-164 | The v0.4.0 reaper deleted the spools the v0.4.0 sweep
   exists to re-adopt; the reaper now leaves `outputs/` to the sweep |
   orchestrator's own reading, 2026-10-29; `collect-gap` reproduction |
   herd-step | done, awaiting a landing | land it; the first consumer's
   runner upgrade is held until it is in a release | nothing |
   `history/2026-10-29-reaper-sweep.md`; `reaper-sweep` |`

3. **A new row, not mine to open:** an automatic end for `.unclaimed` and
   `.foreign-state-root` spools. Proposed text: *nothing ever removes a
   spool the hub stayed silent about; it is renamed, named with its figure
   at every start, and kept until an operator acts. Correct under D19 and
   unbounded in time.* Recommendation: a row at the next free id, owed,
   blocked on Alexa's word, because an automatic delete of a requester's
   evidence is a policy decision. Fallback: leave it in this handoff.

## Resume point

The branch is pushed and green. Next: `herd-merger` lands it as its own
batch, appends the decision above with the next free number, writes the
ORCH-164 row, and the release that carries it is what unblocks the first
consumer's runner upgrade. Nothing else on this branch is waiting.

## Processes

No background process started; no PID to end. The only commands were
`cargo` and the gate scripts, in the foreground, and nothing was killed.
Own build tree `/mnt/ssd_980/herd/build/reaper-sweep` deleted once the
branch was pushed; the gate receipt is kept.
