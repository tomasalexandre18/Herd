# merge-95 ORCH-079 ordering oracle — handoff 2026-10-27

- **Item:** ORCH-079
- **Agent:** herd-merger
- **Branch / commit:** `orch-079-ordering-oracle` / the merge of `4d63361`
  with `origin/main` `c831d55`, based on origin/main `c831d55`
- **PR:** #95

## Done

- **Compared first.** `4d63361` branched at `2ca6dc7`; `origin/main` had
  moved to `c831d55` (PRs #93 and #94). `main` added, since that base,
  `CLAUDE.md`, `docs/disk-policy.md`, a board rewrite of 70 lines, four
  handoffs, `state-root.md`, `workflow.md`, the handoff template,
  `ops/agents/*` and `scripts/check-orchestration.sh` — **no Rust file and
  no `Cargo.*`**, so the merge could not change what the branch's code does.
- **Merged `origin/main` into the branch** (never rebased, never
  force-pushed). **No conflict.** The expected collision on
  `docs/orchestration/board.md` did not happen: #94 rewrote twelve rows and
  `## Next`, this branch edited the single ORCH-079 row, and the two hunks
  are disjoint, so `ort` took both sides without a marker. Verified after
  the merge that every row #94 added (ORCH-093, ORCH-095 to ORCH-107) is
  present and that ORCH-069's narrowed wording survived: `git diff
  origin/main -- docs/orchestration/board.md` showed exactly one changed
  row, ORCH-079's.
- **`docs/decisions.md`: nothing to allocate.** The branch carries no
  `D-NEXT` and adds no entry. Highest number on `origin/main` is **D183**;
  no open PR holds one: `gh pr list`, then `docs/decisions.md` read on
  each of #92, #95, #96, #97 and #98 — none differs from `main`. D170
  stays a gap, as do D143 and D154.
- **Checked the product-build claim, which was the one thing worth
  re-reading.** In `herd-hub/src/durable.rs` the two observation points are
  `#[cfg(not(test))] #[inline(always)]` empty functions, the whole
  `recording` module is `#[cfg(test)]`, and `Synced` is a zero-sized struct.
  `cargo build --workspace` (not a test profile) compiles that half, so the
  recording code is not in `herd-hub` the binary. **No observation cost in a
  release build.** The nine lines of product code are kept deliberately —
  without the `Synced` witness the author's first red run stayed green,
  because the note for a deleted `sync_dir` survived beside the deleted
  sync.
- **Board:** ORCH-079 `active` → `done`, with how it was integrated; `##
  Next` item 3 rewritten so it no longer says an agent holds it.
- **`docs/status.md`:** the remediation dossier is **fourteen** pieces, not
  thirteen; `12` named. Two paragraphs reflowed to stay inside the 90-line
  limit, which `check-docs.sh` enforces. 90 lines exactly.
- The task's own handoff,
  `history/2026-10-27-orch-079-ordering-oracle.md`, exists on the branch.

## Not done, and why

- **`docs/status.md` is not fully rewritten here.** It still says the suite
  is red "on ORCH-069's two assertions and nothing else", which the board no
  longer says, and it still owes the seven-day output floor finding. Both
  belong to one coherent pass, and that pass is made at the last merge of
  this queue rather than four times.
- Nothing of the durability tier is touched. ORCH-083 and ORCH-084 remain
  owed; this PR only puts the gate they must stay green against on `main`.

## Gates

All four Rust gates plus `check-docs.sh` and `check-orchestration.sh`, in
this merger's own worktree, through `nix develop --command`. The diff
touches Rust, so none was skipped.

```text
$ nix develop --command cargo build --workspace
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 22.45s
(exit 0)

$ nix develop --command cargo fmt --all --check
(no output, exit 0)

$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 12.84s
(exit 0)

$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
(exit 0)

$ ./scripts/check-orchestration.sh
check-orchestration: 138 passed, 0 failed
(exit 0)
```

The oracle itself, from the suite run:

```text
test store::ordering_oracle::every_transition_fsyncs_the_record_and_its_directory_before_appending_its_event ... ok
test store::ordering_oracle::no_writer_of_a_record_or_an_event_was_added_without_the_oracle_seeing_it ... ok
```

`herd-hub` lib: 189 tests, all passing (184 on the branch before `main` was
merged in; the difference is `main`'s, not this branch's).

**`nix develop --command ./scripts/test.sh` is RED on this workstation,
exit 101, for host reasons.** Three targets, one test each:

```text
error: 3 targets failed:
    `-p herd-hub --test admission`
    `-p herd-hub --test killed_hub`
    `-p herd-hub --test slots`
test suite: FAILED (cargo test exited 101)
EXIT=101
```

**Skips (D27): at least 12**, each printed as a skip, none counted as a
pass; all twelve are the cgroup-v2-delegation family of `herd-runner --test
confinement`. The suite says itself that the figure is **a floor and not a
count** because the run did not finish (D179):

```text
 AT LEAST 12 assertion(s) COULD NOT RUN on this host and were
 skipped, and the suite did not finish (cargo test exited 101),
 so this list is a floor and not a count.
```

## Risks and findings

- **How the three failures were attributed, and it was not by assertion.**
  Two means, both independent of what the failures say about themselves.
  **(1) By construction.** The only Rust this branch changes outside
  `#[cfg(test)]` is five calls into `watch::returned` and `watch::issuing`.
  An integration test under `herd-hub/tests/` links `herd_hub` as a
  *dependency*, which is compiled without `cfg(test)`, so for all three
  failing targets those two functions are the empty `#[inline(always)]`
  bodies and the `recording` module does not exist. No integration test can
  observe this branch. **(2) By a base run** of the same three targets at
  `origin/main` `c831d55` in this same worktree, recorded below.
- **One of the three, `killed_hub`'s
  `a_result_the_hub_cannot_write_is_kept_until_it_can`, is ORCH-107
  exactly**: `real_herd/mod.rs:649`, *"herdctl […] never
  came back"* — a timeout, not a wrong value.
- **`slots.rs:744`, `the_derived_j_is_recorded_and_is_what_the_job_is_given`,
  is ORCH-069 exactly.** Not claimed green on CI: `MemTotal` there is 15983
  MiB, under the guard's 17408, so CI **skips** it and by D27 a skip is
  never a pass.
- **`admission.rs:419` is a finding, and it argues ORCH-069 was narrowed too
  far.** It failed with a *figure*, not a timeout — *"lender admits at most
  4 GiB of memory (5 GiB lent, 1 GiB kept as its own floor) and this job
  needs 5 GiB"* — which is ORCH-069's construction, a bound derived from
  this host's live `MemTotal` against a constant written for a larger
  machine. The board now names ORCH-069 as `slots.rs:744` **alone**, and
  that is one site short.

## Decisions needed

None for this PR.

## Resume point

PR #97 (`orch-094-id-highwater`) is next in the queue, and **no CI run has
ever started for it** — `gh run list --branch orch-094-id-highwater` is
empty, so its green is unverified rather than unverified-and-fine. Merge
`origin/main` into it, push, and read the run that push creates.

## Processes

Background shells started and ended by this agent: one `scripts/test.sh`
run (exit 101) and one wait loop. No process of another agent was signalled;
no `pkill`, no kill by name.
Own `target/` is not deleted at this merge: the same worktree carries the
three integrations after it. It is deleted once the last of them is
pushed, and the queue's final handoff records it with its path.
