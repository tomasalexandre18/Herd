# measured-fold — handoff 2026-10-29

- **Item:** ORCH-186 (the fold it left owed), and the `herd-hub report`
  all-unread defect, which has no row yet
- **Agent:** herd-step
- **Branch / commit:** `measured-fold` / `bb359c4` plus this file, based on
  `origin/main` `1a8c1ae`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

**One body for the measured-bytes rule.**
`herd_proto::size::measured_bytes(bytes: u64) -> String` is the only copy
now, beside `show_bytes` in `herd-proto/src/size.rs` — the two rules in one
file, which is where a reader looking for either would look. The three
bodies it replaces were character-identical in the two hub modules and
character-identical in the runner:

- `herd-hub/src/report.rs` `bytes_said` — removed; the module imports
  `herd_proto::size::measured_bytes` and its 19 call sites now name it.
- `herd-hub/src/retention.rs` `said_bytes` — removed, same import, 7 call
  sites.
- `herd-runner/src/size.rs` `measured_bytes` — removed; the module is now
  a re-export (`pub use herd_proto::size::{measured_bytes, parse_bytes,
  show_bytes};`), so `sweep.rs` and `outputs.rs` are untouched and still
  call `crate::size::measured_bytes` by that name.

**`show_bytes` is byte-for-byte unchanged** and the asymmetry is now
documented beside both bodies rather than in three places: the typed rule
returns the first unit a figure divides *exactly*, which is right for a cap
an operator typed (`200GB` → `200 GB`, D148, D152) and wrong for a
measurement, because it makes the unit a property of the number.

**The two rules are still distinguishable, and a test makes erasing one
red.** `herd-proto/src/size.rs`
`the_typed_rule_and_the_measured_rule_disagree_on_purpose` asserts both
readings of the same three figures and then asserts they differ:

| figure | `show_bytes` (typed) | `measured_bytes` (measured) |
|---|---|---|
| 535 691 000 | `535691 kB` | `535691000 bytes (510.87 MiB)` |
| 200 000 000 000 | `200 GB` | `200000000000 bytes (186.26 GiB)` |
| 67 108 864 | `64 MiB` | `67108864 bytes (64.00 MiB)` |

The consumer's own golden line and the defect's whole class moved with the
function and still read literally, with their "do not paste the new output
over this expectation" notes intact. The typed-cap test stays:
`show_bytes(parse_bytes("200GB"))` is `200 GB`, `64MiB` is `64 MiB`,
`2GiB` is `2 GiB`.

**`herd-hub report` no longer prints a page of arithmetic about nothing.**
When `census.read == 0 && !census.unread.is_empty()`, the records section
now ends with the shape the unreadable `jobs/` directory already had:

```
  NOT ONE RECORD COULD BE PARSED BY THIS BUILD, so every figure below is
  about no records at all and about nothing else: the jobs directory was
  read, the records are there, and not one of them became a figure.
```

It stays a report: no exit code, no refusal, no early return, every section
still printed, and the code comment says why — `report` exists to be
runnable on a state root a hub will not open, two published release pages
say so, and an operator reaches for it precisely when the records cannot be
read.

**An absent `record_version` is now said to be absent**, in both readers.
`report.rs`'s `RecordOutputs` and `retention.rs`'s `VersionProbe` hold
`Option<u32>`, so a `job.json` with no such key reads as `None` and the
refusal says *"it carries no record version at all, and this build reads
version 1"* instead of describing a missing field as a field that says
zero. A record that really says `0` still reads as a version, because zero
is a thing a record can say (`job.rs` draws the same line on
`at_its_ceiling`).

**Ten names out of N now say which fraction they are.** The bound stays at
`NAMED_AT_MOST` = 10; the bare `and N more` under the unread list is
replaced by

```
    ^ these are the first 10 of 13, in job-id order. The rest are
      later in `ls jobs/` than the last name above; this report names
      no more of them and no other file lists them.
```

**Six new tests, all in the source files this task owns** (see *Not done*
for why none of them is in `tests/`): five in `herd-hub/src/report.rs`
(the banner fires and the report is still a report; a root with no records
*yet* is not a failure; a root this build reads carries no banner at all;
absent-versus-zero; the fraction line) and one in
`herd-hub/src/retention.rs` (`VersionProbe` reads absent as absent). Their
hand-written fixtures spell the version as `RECORD_VERSION` and
`RECORD_VERSION + 1`, never a literal `1`, so a bump of that constant
cannot quietly turn "a record this build reads" into its opposite.

**The record-version prediction is measured, with its control.** See
*Risks and findings*.

## Not done, and why

- **`herd-hub/tests/state_root_report.rs` and
  `herd-hub/tests/retention_preview.rs` are untouched**, on the
  orchestrator's mid-task narrowing: another agent is spending
  `RECORD_VERSION` 1 → 2 and owns both files. The four report tests were
  written there first and were moved into `report.rs`'s own `mod tests`;
  `git checkout --` put the integration file back and `git status` was
  clean of it before the commit. Both files still pass unchanged (18
  tests), which is itself the evidence that the fold changed no printed
  byte.
- **The retention preview's *printed* absent-version sentence is not
  pinned.** The field semantics are (`VersionProbe` unit test), and the
  wording is the same string literal as the report's, which is pinned. The
  rendered line belongs in `retention_preview.rs`, which is the other
  agent's file.
- **`herd-proto/src/lib.rs` is untouched**, as the brief required: no
  change was needed, `size` is already a declared `pub mod`. The crate root
  re-exports `parse_bytes` and `show_bytes` but not `measured_bytes`, so
  callers name `herd_proto::size::measured_bytes`. Adding it to that
  re-export list is a one-line follow-up for whoever owns `lib.rs` next.
- **The retention preview has no all-unread banner and no fraction line**
  of its own; only `report` was in the brief. Both are the same shape in
  the same crate.
- **`docs/decisions.md` and `docs/orchestration/board.md` are untouched**;
  the decision entry is drafted below.

## Gates

Run **before** this file was committed, on the tree of `bb359c4`:

```
$ nix develop --command cargo fmt --all --check
(no output)
$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 31.62s
$ nix develop --command cargo build --workspace
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 10.87s
$ nix develop --command ./scripts/test.sh
test result: ok  x64 suites, 950 passed, 0 failed
 14 assertion(s) COULD NOT RUN on this host and were skipped.
test suite: PASSED
$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
```

Skips, all 14 host-shaped and none of them mine: twelve need a delegated
cgroup v2 subtree (`confinement`, the io/cpu/pid ceilings, the pressure
figures, the announced-room tests) and two are the load-gated benches
`flood-fifty` and `flood-two-fifty`, which read the host's own load and
declined. Copied as skips, counted as nothing (D27).

The **last run**, on the tree that carries this file:
`./scripts/gate.sh`, bare, with `CARGO_TARGET_DIR` set to
`/mnt/ssd_980/herd/build/measured-fold`.
Its output goes to `target/herd-gate/gate-<stamp>-<sha>.log` and its
receipt beside it; the verbatim verdict is in the agent's report, not in
this tree.

A **first** `./scripts/gate.sh` run on `bb359c4` went **RED** at step 6 on
a host condition and is reported red: five `real_herd` tests failed because
`/chatons.slice` had 361 MiB left under `memory.high`, so the runner under
test correctly refused to start (`--host-memory-floor is 512 MiB and this
host lends 361 MiB`), with host load 15.12 from sibling builds. Nothing was
edited in response; the branch waited for room.

## Risks and findings

1. **The record-version prediction is confirmed, with its control.** Both
   runs used one throwaway root of three well-formed records in this
   agent's scratchpad, each with real bytes under it, and the same debug
   binary built from this branch.
   - *Control*, `RECORD_VERSION` = 1: `job directories 3`, `records read
     3`, `records this build could not read 0`, section 1 `r_file 1.00`
     over `27 bytes`, section 2 `population 3 records`. **Exit 0.** The
     fixture is well-formed and the records read.
   - *Bumped*, the same root, the constant edited to 2 and the binary
     rebuilt: `records read 0`, all three named with *"it carries record
     version 1, and this build reads version 2"*, the new banner, then
     sections 1 to 3 in full — `r_file undefined: no record names a
     digested byte`, `population 0 records`, `every files/ directory
     measures 0 bytes`. **Exit 0.**
   So the prediction held exactly, and the banner is the only thing
   standing between that screen and an operator reading it as an answer.
   The edit to `herd-hub/src/job.rs` was reverted immediately and is **not
   in the commit** (`git status` clean, `RECORD_VERSION` back to 1).
   **This is the shape the `RECORD_VERSION` 1 → 2 bump now in flight will
   create on every existing state root**, measured rather than predicted.
2. **A figure in section 3 is false on an all-unread root, and it is not
   this branch's doing.** `every files/ directory measures 0 bytes` was
   printed while the fixture's `jobs/*/files/` held 27 bytes: the
   measurement happens inside the per-record loop, after the version
   check, so a root whose records are all unread measures nothing and says
   `0`. The banner above it now says every figure below is about no records
   at all, which makes the screen honest, but the line itself still states
   a measurement that was never taken. Fixing it means measuring `files/`
   outside the record loop, which changes section 3's arithmetic and wants
   its own brief and row.
3. **`NAMED_AT_MOST` still truncates five other lists** in `report.rs`
   (digest conflicts, claimed-but-absent, claiming-more-than-held,
   removed-but-present, unfinished collections) and one in `retention.rs`,
   all with the bare `and N more`. Only the unread list was in the brief;
   the shortfall list's `and 3 more` is asserted in
   `the_shortfall_list_names_the_ten_largest_and_counts_the_rest`, in the
   test file this task may not touch, so making the six consistent needs
   that file and belongs with whoever owns it.
4. **Nothing in the brief's premises was refuted.** The three bodies were
   where it said, identical as it said, `show_bytes` is the other rule as
   it said, and `herd-proto/src/lib.rs` needed no edit. One thing the brief
   did not mention: the **same** absent-versus-zero defect was in
   `retention.rs`'s `VersionProbe`, so it was fixed in both readers rather
   than one.

## Decisions needed

Draft entry, for `herd-merger` to number and append:

```markdown
- **D-NEXT** (2026-10-29) — **A byte figure is formatted by who produced
  it, not by what it divides by, and the two rules live in one file.**
  `herd_proto::size::show_bytes` is for a figure a human **typed** and
  returns it in the unit family they typed it in (D148, D152);
  `herd_proto::size::measured_bytes` is for a figure Herd **measured** and
  prints the exact count always, with a binary approximation beside it and
  never instead of it. A new byte figure picks its formatter by answering
  one question — did a human type this, or did we measure it? — and never
  by how the number happens to divide. The measured rule had three
  identical bodies (ORCH-186); it now has one, beside the typed one, and
  the asymmetry between them is documented where both are. Erasing the
  difference is a red test, not a tidy.
```

No question is open for Alexa: nothing here changes what the product is or
promises.

## Resume point

`herd-merger`, at the next batch: take `measured-fold` at the sha this
file's commit produced. Nothing is sequenced after it inside this task.
For the orchestrator: finding 2 (section 3's untaken measurement printed
as `0`) and finding 3 (five more lists with a bare `and N more`) want rows.

## Processes

PIDs started and ended: `cargo`/`rustc` under
`nix develop --command` for the four Rust gates, one
`cargo build -p herd-hub --bin herd-hub` twice for the measurement, two
runs of the built `herd-hub report`, one `./scripts/gate.sh`, and one
background shell polling `/sys/fs/cgroup/chatons.slice/memory.current`
while the slice was tight. All exited; none was killed by name, nothing
under `/sys/fs/cgroup` was written and no cache was dropped.

`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/measured-fold`, outside the
worktree, deleted once the branch is pushed. The throwaway state root
`<scratchpad>/measure-root` is the agent's own and holds no project data.
