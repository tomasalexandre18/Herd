# m12-p2b-pack-status — handoff 2026-10-30

- **Item:** ORCH-251, **closed**; ORCH-256 **narrowed** to the runner's
  `packs/refused/` alone (its hub half is this task's)
- **Agent:** herd-step
- **Branch / commit:** `m12-p2b-pack-status` / `8827416` (this handoff's
  commit follows it), based on origin/main `b99d32d` (batch 20)
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

### `herd-hub/src/server.rs` (+92 / −8): the three fields stop being dropped

1. **The arm was not where either document said it was.** ORCH-251 cites
   `:4692-4694`; P2a's handoff cites `:4871-4873` on `a8cf397`. On
   `b99d32d` it is **`:4912-4923`** — L2's `holders_of_what_it_needs`
   (+167) moved it again. Found by reading, not trusted.
2. **One value, not three parameters.** `PackStoreSaid` (`:1521`) holds
   the three `Option`s with the state table on its doc comment, so
   nothing downstream can take two of the three and believe it has the
   fact. The Health arm binds the three names (`:4981`) and builds it
   (`:5010`).
3. **Kept on the connection**: `ConnectedRunner::pack_store` (`:1392`),
   initialised all-absent at the handshake (`:1896`), set by
   `record_health`'s new parameter (`:2417`, assigned `:2454`).
4. **Replaced whole on every report, deliberately not `load_milli`'s
   rule.** `load_milli` keeps the last figure that could be read,
   because an unreadable load is a *failure to measure* something that
   is certainly there (E-32). An absent pack figure is the opposite: it
   is the runner **saying** which of three states its store is in. A hub
   that kept the last measurement would print a figure nobody claims any
   more and would hide the no-store state, which is the one state an
   operator most needs — a runner holding gigabytes and saying nothing.
   Both rules are now written beside each other in the code.
5. **Into the document field for field** (`:2112`), with no flattening:
   the mapping is three `Option`s copied, so absence survives to the
   JSON.

### `herd-hub/src/status.rs` (+347 / −2): the document and the screen

1. **Three fields on `RunnerNow`** (`:285`...), flat and named exactly as
   the wire names them, each `skip_serializing_if = "Option::is_none"` —
   so an absence is **absent from the JSON, not `null`**, which is the
   property P2a proved on the wire held one hop further on.
2. **`STATUS_VERSION` 1 → 2** (`:105`), and the reason is not
   compatibility. The fields are additive and serde would have read an
   old document without them. It moved because at version 1 a renderer
   finding no `packs_held` cannot tell whether *the runner did not say*
   or *the hub that wrote this does not record it* — and "the runner did
   not say" is the whole point of these three fields. At 2 there is one
   reading left. The cost is named on the constant: an operator's older
   build now refuses the document by name with both figures
   (`Status::read`), which is what this number exists to do.
3. **One line per runner, four sentences** (`pack_store_said`, `:548`),
   because the absences are three different facts:

   | what the runner said | the line |
   |---|---|
   | all three | `4608 bytes (4.50 KiB) of 200 GB allowed, in 3 pack(s)` |
   | the budget alone | `it did not say what it holds; its owner allowed 200 GB` |
   | measurements, no budget | `4608 bytes (4.50 KiB), in 3 pack(s); its owner declared no budget, so there is nothing here to weigh that against` |
   | nothing | `it did not say` |

   A fifth arm names each field and its own absence, for a combination
   the wire permits and no build sends.
4. **`did not say`, not `unknown`** (`DID_NOT_SAY`, `:518`), and the
   module header now carries the distinction: `unknown` is the hub's own
   ignorance about a figure that is certainly there; this is said with a
   health report in hand, and it is a fact about the machine. The plan's
   *Done when* asks for these words.
5. **The right renderer, which is ORCH-186.** The use goes through
   `measured_bytes` (exact count, binary approximation beside it); the
   budget through `show_bytes` (the family its owner typed it in). The
   test asserts the negative too: a budget typed as `200GB` must not
   read back as `200000000000 bytes (186.26 GiB)`.

### `herd-hub/src/report.rs` (+488): section 4, the packs this hub holds

`report.rs` and `status.rs` held **zero** occurrences of `pack` before
this. The new section is measured, never claimed — there is no manifest
of manifests, so every figure is `directory_bytes` over a real
directory, the sweeper's own walk, for the reason figure 3 uses it.

| line | what it is |
|---|---|
| `packs registered` | directories under `packs/manifests/`, with their summed file lengths |
| `registrations interrupted` | `packs/manifests.part/`: written, never renamed into place, nothing reads them, the next hub start removes them by name |
| `everything under packs/` | the **whole directory**, measured once |
| `neither of the two above` | the difference, printed when there is one |

The total is measured over `packs/` rather than summed from the two
figures above it, **so a child this build has never heard of is still
counted**. That is the property the section exists for: the pack
population went uncounted for a milestone because nothing measured the
directory it was in, and a total summed from known children would repeat
that one directory further down. A root with **no** `packs/` says so in
a banner rather than printing nought packs, which is section 3's own
rule for an unreadable `jobs/`.

Nothing is removed and nothing is weighed: **who owns `packs/`'s growth
is ORCH-215, which is Alexa's question**, and the section's own prose
says so. §0.7 asks only that what is kept appear with its size.

### Tests

**`herd-hub/tests/pack_status.rs` (+331, new file)** — a real hub, a real
runner, the document on the disk and the shipped `herd-hub status`:

| test | what it holds |
|---|---|
| `the_hub_shows_a_real_runners_pack_use_against_its_budget` | a store planted before the runner starts (4608 bytes in 3 packs) and `--pack-budget=200GB`: the typed document, the raw JSON keys, and the screen's line to the byte |
| `a_runner_whose_pack_store_will_not_open_is_shown_as_having_said_nothing` | a regular file where `packs/` must be a directory, so `PackStore::open` refuses: all three absent in the document, **absent and not null** in the JSON, `its pack store: it did not say` on the screen, the runner still in the fleet, and its own stderr line |

The second state is the **only** silence a real binary can be made to
show: every runner opens and measures its store at the dial, so an empty
store honestly sends `0`. It is also P2a's finding 1 — the failure mode
is a machine holding gigabytes that says nothing.

**Nine unit tests** in `status.rs` (4) and `report.rs` (5), including
`the_report_walks_the_directory_the_registry_publishes_into`, the guard
on the one name this task spells twice (see *Risks* 3).

### Docs

- **`docs/orchestration/board.md`**: ORCH-251 → `done`, with the real
  line numbers, the two sentences the row got wrong, and the evidence;
  ORCH-256's *Next action* amended — the hub half done, the row narrowed
  to the runner's `packs/refused/`, State stays `owed`.
- **`docs/status.md`**: item 1's stale ORCH-251 citation replaced.
  Four lines in, four lines out — the file stays at **90**, which is
  `check-docs.sh`'s own limit, so **no line elsewhere had to be
  reflowed**. The brief expected tail slack; there is none, the limit is
  90 and the file was at 90.

## Not done, and why

1. **The runner's `packs/refused/` is still in no report** — ORCH-256's
   other half, on `herd-runner`, which this task does not own. The row
   is narrowed to it rather than closed.
2. **Nothing evicts, nothing is weighed.** The hub shows the use against
   the budget and compares them against nothing: that is the plan's own
   reviewer line and invariant 2's shape at the disk. P3 is the gc.
3. **`herdctl`'s fleet listing says nothing about packs.**
   `RunnerStatus`/`RunnerFacts` are untouched: the plan's *Done when*
   names `status.json` and `herd-hub status`, and widening a protocol
   struct is ORCH-235's exposure for no asked-for gain.
4. **The hub's own output-budget line still prints its figure raw** —
   see *Risks* 4. A second ORCH-186 site, in a file I own, that nothing
   in this brief asked for.
5. **No `docs/decisions.md` entry and no new board row.** The brief
   asked for neither; `decisions.md` is `herd-merger`'s alone.
6. **`docs/coverage.md` untouched** — see *Risks* 6.

## Gates

**Run before this commit, on the code commit `8827416`'s tree**, all four
Rust gates under `nix develop --command`, with
`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/m12-p2b`. **Each one bare to a
file, never through a pipe** — `${PIPESTATUS[0]}` is not a cure and the
mechanism is measured (see *Risks* 8 for the shape it took here).

```
=== cargo build --workspace
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.25s
BUILD EXIT: 0
=== cargo fmt --all --check            (bare, output empty)
FMT EXIT: 0
=== cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 8.09s
CLIPPY EXIT: 0
```

`clippy --all-targets` was run **after** the last new test body was
written, which is this branch's answer to ORCH-235: it compiles every
test crate in the workspace, `herd-hub/tests/pack_status.rs` included.

```
=== nix develop --command ./scripts/test.sh
ignored census: COMPLETE (38 ignored, 0 without a reason)
test suite: PASSED
SUITE EXIT: 0
summed from the per-target lines: 1187 passed, 0 failed, 38 ignored
skips: 12
```

The 38 `#[ignore]`s were counted with a start-of-line anchor, which is
what `scripts/test.sh` itself uses; a loose grep reports 51.

**The 12 skips, by name** (D187: a list, never a count). All twelve are
the standing cgroup-delegation set and **not one is new**:
`a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device`,
`a_declared_ceiling_changes_what_the_inert_weight_line_says`,
`a_job_cannot_fork_past_its_own_pid_ceiling`,
`a_job_s_cgroup_carries_the_cpu_io_and_pid_bounds`,
`a_job_s_cgroup_carries_the_hard_io_ceiling_it_was_given`,
`a_job_s_record_carries_the_pressure_it_ran_under`,
`a_tighter_enclosing_memory_max_lowers_the_room_that_is_announced`,
`an_allocation_past_the_ceiling_kills_the_job_and_only_the_job`,
`an_io_weight_that_cannot_bite_is_said_out_loud`,
`no_declared_ceiling_leaves_io_max_empty`,
`no_declared_quota_leaves_cpu_max_at_the_kernel_s_max`,
`the_runner_says_which_memory_ceiling_it_enforces`.

**The two load-gated flood benches did not decline this time** and both
ran green — `fifty_assignments_from_a_hub_that_is_not_one_...` and
`two_hundred_and_fifty_assignments_...`, 2 passed in 5.27s. P2a saw them
decline as a delta at load **9.92**; this run started at load **4.86**
against their ceiling of 8. That is the host and not the diff, in both
directions, so the skip list is **12** here and **14** there and neither
is a regression. D27: printed as skipped, never counted as passed.

**The 11 new tests all ran and all passed**, named so the list is
checkable rather than summarised: `report::tests::` —
`the_report_names_the_pack_population_with_its_size`,
`a_root_with_no_packs_directory_is_not_an_empty_registry`,
`a_registration_that_was_interrupted_is_counted_apart_from_the_packs`,
`bytes_under_packs_that_no_figure_names_are_still_in_the_total`,
`the_report_walks_the_directory_the_registry_publishes_into`;
`status::tests::` —
`the_screen_prints_one_runners_pack_use_against_its_budget_with_both_figures`,
`a_runner_that_said_nothing_about_its_pack_store_says_so_and_never_nought`,
`a_pack_figure_a_runner_did_not_give_is_absent_from_the_json_and_not_null`,
`a_pack_store_that_grew_is_news_about_the_fleet`; and against the real
binaries `the_hub_shows_a_real_runners_pack_use_against_its_budget` and
`a_runner_whose_pack_store_will_not_open_is_shown_as_having_said_nothing`.

**The first full suite run was RED**, on one test, and it is reported red
rather than smoothed over:
`every_figure_of_the_report_is_this_fixture_to_the_byte`
(`herd-hub/tests/state_root_report.rs`), *1 failed, 6 passed*, because it
asserts the report's whole text and the report had grown a section. Fixed
by extending the fixture — see *Risks* 1 for why that is not an
expectation edited to go green.

**Every new assertion was demonstrated red before it was trusted**, five
breaks, the three source files and the new test file restored to
**byte-identical** sha256 after each one (`sha256sum -c`, all four
checked after every restore):

| the break | what went red |
|---|---|
| `record_health` assigns `PackStoreSaid::default()` — the hub discarding the fields again | `the_hub_shows_a_real_runners_pack_use_against_its_budget` alone, timing out on a document that never showed a figure. **The "did not say" test stayed green**, which is the pair's whole point: a discarding hub is indistinguishable from a silent runner, so one test cannot hold both claims |
| `measured_bytes` and `show_bytes` swapped on the use and the budget (ORCH-186) | the unit test **and** the end-to-end one, with the real symptom printed: `left: "… 4608 bytes of 200000000000 bytes (186.26 GiB) allowed …"` against `right: "… 4608 bytes (4.50 KiB) of 200 GB allowed …"` |
| the snapshot maps `Some(bytes.unwrap_or(0))` and `Some(held.unwrap_or(0))` — a nought standing in for a silence | `a_runner_whose_pack_store_will_not_open_…` alone, `left: Some(0)`, `right: None` |
| `PACKS_MANIFESTS_DIR` drifted to `"manifest"` — the duplicated name going stale | `the_report_walks_the_directory_the_registry_publishes_into` alone, naming the directory the registry really made. **The population test stayed green**, because it plants through the same constant — which is exactly why the registry guard exists |
| `pack_store_said`'s budget-alone arm returning `it did not say` — two states flattened into one | `a_runner_that_said_nothing_about_its_pack_store_says_so_and_never_nought` alone, on *"a budget with no measurement beside it is still a fact"* |

**And the binary was run and its output read as a human**, which is
ORCH-186's own lesson — that defect was caught by reading a screen and no
gate saw it. `herd-hub report` was run against three hand-made roots:
one with three packs, a residue directory and a stranger directory; one
with no `packs/` at all; one with an alien file under `manifests/`. Two
things came out of reading rather than asserting: the column alignment of
two lines was wrong (`everything under packs/` and `neither of the two
above` were at column 43 and 40 against the section's 39) and is fixed,
and the alien-file line reads correctly as *not* part of the pack count.
The full runner block of `herd-hub status` was read the same way, with
the pack line in place between `lends` and the capability list.

**The last run, named and not quoted:** `./scripts/check-docs.sh`, run
bare on the tree this handoff is committed into, its output going to
`check-docs-last.log` in this agent's scratchpad (the full path is in the
agent's report) and verbatim in that report. `./scripts/gate.sh` was
**not** run: the batch's gate is `herd-merger`'s.

## Risks and findings

1. **I wrote one file outside this task's stated *Owns*, and it was
   unavoidable.** `herd-hub/tests/state_root_report.rs`'s
   `every_figure_of_the_report_is_this_fixture_to_the_byte` asserts the
   report's **whole text**, so a new section reddens it by construction.
   The first full suite run was **red on exactly that one test and
   nothing else**. I extended the fixture with section 4's text — the
   banner, because that root has no `packs/` — and added a doc sentence
   saying why it is the banner and where the figures are asserted. This
   is not an expectation edited to hide a defect: the text changing *is*
   the deliverable, and the test is a fixture of it. **Say so to whoever
   briefs P6**, which also owns `status.rs`. No other branch in flight
   touches that file (P3 is on `herd-runner/src/packstore.rs`).
2. **`STATUS_VERSION` moved to 2 and that is a judgement, not a
   mechanical consequence.** The brief said to move it *only if the
   document's shape changes*. Three additive `Option` fields do not
   break a reader — so the argument for moving it is not
   compatibility, it is that **an absence at version 1 is ambiguous**
   between the runner's silence and a hub that does not record the
   figure, which is precisely what these fields exist to make
   unambiguous. The cost is real and one-sided: an operator running an
   older `herd-hub status` against a new hub's root now gets a refusal
   by name instead of a screen with no pack line. Reversible in one
   constant plus three test strings if the orchestrator disagrees.
   `STATUS_VERSION` had **never** moved before (`git log -S`, one
   commit: `1280e69`, which introduced it at 1).
3. **`report.rs` now spells `manifests` a second time, and the
   duplication is guarded, not hidden.** `registry.rs`'s `MANIFESTS_DIR`
   is private to that module and `PACKS_DIR` is `pub(crate)`; the report
   may not open a `PackRegistry` to ask, because opening one *creates*
   directories, probes them for writability and removes residue — on a
   root this report is only allowed to read. So the name is written again
   in `report.rs` and
   `the_report_walks_the_directory_the_registry_publishes_into` opens a
   real registry on a **temporary** root and asserts the report's path is
   the directory it made. Demonstrated red by drifting the copy to
   `"manifest"`: the population test stayed **green** (it plants through
   the same constant), and only the guard went red — which is why the
   guard exists. **A one-line change to `registry.rs` making
   `MANIFESTS_DIR` `pub(crate)` would remove the duplication**; it is
   outside this task's *Owns* and P6 owns that file, so it is a finding
   and not an edit.
4. **A second ORCH-186 site, in a file I own, left alone on purpose.**
   `status.rs:601` prints `the output budget is {} bytes` with the raw
   `u64`, so a hub given `200GB` prints `the output budget is
   200000000000 bytes`. D148 and `show_bytes`'s own doc say a figure its
   owner typed must read back as `200 GB`. Nothing asserts that text, so
   the fix is one line plus nothing. I did not make it: it is an
   operator-facing line this brief did not ask about, and scope creep
   inside an owned file is still creep. **It did force one test change**
   — the pack assertions read the pack-store *line*, not the whole
   screen, because `screen.contains("200000000000 bytes")` was asserting
   about somebody else's sentence. The comment in the test says so.
5. **What I added to `server.rs`, for the next assembler.** L2 inserted
   `holders_of_what_it_needs` (+167) in batch 20 and **P6 is the writer
   after me**. This branch adds, in file order: `ConnectedRunner::
   pack_store` (`:1392`), `struct PackStoreSaid` immediately before
   `struct Peers` (`:1521`), one line in the handshake's `ConnectedRunner`
   literal (`:1896`), three lines in the status snapshot's `RunnerNow`
   literal (`:2112`), one parameter on `record_health` (`:2417`) and one
   assignment in its body (`:2454`), and in the `ToHub::Health` arm the
   three bindings (`:4981`) and the value they build (`:5010`). P6 wants
   the same `RunnerNow` literal at `:2112` and the same `status.rs`
   `RunnerNow` struct, so **P6 rebases on this, not beside it**.
6. **`docs/coverage.md` E-35's cell can be misread now, and I left it.**
   It says *"What M13 still owes is E-37's remainder and the
   use-against-budget figure, which stays `herd-hub report`'s because it
   is a walk of the volume"*. That is the **hub's output** budget, and it
   is still true. This branch adds a **runner's pack** use-against-budget
   to the document — a different budget, measured by the runner, no walk.
   The cell is not false; it is one word away from reading as if it were.
   `coverage.md` is not in this task's *Owns*.
7. **ORCH-235 exposure: none from this branch's own file, and no live
   branch collides.** `herd-hub/tests/pack_status.rs` constructs no
   protocol struct at all — it reads `herd_hub::Status` and spawns the
   real binaries — so a branch that widens `Welcome` or `Health` cannot
   give it `E0063`. What it *does* widen is `status::RunnerNow`, a
   hub-internal struct with one construction site (`server.rs:2112`) and
   one test fixture (`status.rs:786`). Measured: P2a is **already in
   `origin/main`** at `b99d32d` (`git merge-base --is-ancestor`), and of
   every pushed branch not yet in `main`, none of `m12-honest-failures`,
   `m12-l2-locality`, `m12-r1-free-space-guard`, `m12-r0-r9-preview` or
   `m12-plan-rows` touches `herd-hub/src/server.rs`, `status.rs`,
   `report.rs` or `herd-hub/tests/state_root_report.rs` — the only
   branches that do are the superseded `m2`/`m3`/`m9`-era ones. P3, the
   other heavy agent, is on `herd-runner/src/packstore.rs` and had not
   pushed when this was measured.
8. **A background exit code lied to me, and the mechanism is the one the
   brief names.** My first suite run was launched with
   `… > log 2>&1; echo "SUITE EXIT: $?"` and moved to the background by
   the harness; the harness then reported *"completed (exit code 0)"*,
   because the last command in the chain was the `echo`. The real status
   was in the log — `SUITE EXIT: 101` — and the one failing test was
   three lines deep in `target/herd-cargo-output.txt`, because
   `scripts/test.sh` sends cargo's stdout through `tee` to that file and
   the harness's own task-output file kept only a tail. **A backgrounded
   gate is read from its own log file and from
   `target/herd-cargo-output.txt`, never from the harness's summary
   line.** Same family as the `${PIPESTATUS[0]}` row; no new row, because
   it is that row's mechanism in a new place.
9. **The document is not state a test may checksum (D215), and nothing
   here does.** Every wait in the new test is on what the document
   *says*; `real_herd::state_bytes` is untouched. The three fields change
   on every health report, so they also make `same_but_for_the_clock`
   differ — asserted, because that is what makes them *arrive* rather
   than sit in memory.
10. **A use over its budget is printed as two figures and no verdict.**
    The screen says `… of … allowed` and never *"over budget"*. That is
    the plan's reviewer line — the hub shows, it never enforces someone
    else's disk — but an operator scanning a fleet would be helped by a
    marked line. It is a presentation choice, not a mechanism; worth a
    row if anyone wants it, and it must stay a statement and never a
    gate.

## Drafted for `herd-merger`

**No `docs/decisions.md` entry.** The brief asked for none. The one
judgement that might have wanted one is `STATUS_VERSION` 1 → 2, and it is
recorded where it belongs — on the constant's own doc comment, in full,
with its cost — rather than as a decision about *what the product is*.
If the orchestrator wants it in `decisions.md`, it is a one-paragraph
entry and I have not drafted it, because drafting an entry nobody asked
for is how a number gets allocated for a code comment.

**No new `board.md` row.** Both rows this task touches are **amended on
this branch**, so the merger has nothing to paste:

- **ORCH-251** → State `done`, its *Next action* carrying P2b's
  commit, the real line numbers, the `STATUS_VERSION` move with its
  reason, the two renderers, and the five red demonstrations; its
  *Evidence* gains `PackStoreSaid`, `pack_store_said`, `STATUS_VERSION`,
  `herd-hub/tests/pack_status.rs` and this handoff.
- **ORCH-256** → State stays `owed`, narrowed in its *Next action* to the
  runner's `packs/refused/` alone, with section 4's four lines, the
  banner rule, the figure read as a human
  (`packs registered 3 pack(s), 24576 bytes (24.00 KiB)`) and the guard
  on the duplicated name; its *Evidence* gains `Packs`,
  `read_the_packs`, section 4 and this handoff.

Three corrections the merger may want in its own record, each measured
here:

1. ORCH-251's *Item* cell cites the hub's discard at
   `server.rs:4692-4694`. On `b99d32d` it is **`:4912-4923`**. P2a's
   handoff says `:4871-4873` on `a8cf397`. The arm is the same one; the
   numbers moved twice on 2026-10-30.
2. **`herd-hub/tests/state_root_report.rs` is edited on this branch**
   although it is outside the brief's *Owns* — see *Risks* 1. It is the
   whole-text fixture of the report, and a new section reddens it by
   construction.
3. `docs/status.md` has **no tail slack**: `check-docs.sh`'s limit is 90
   and the file is at 90. Item 1 was replaced four lines for four.

## Decisions needed

**None blocking.** Two things for the orchestrator's eye, each with my
recommendation, neither a question for Alexa:

1. **`STATUS_VERSION` 2** (*Risks* 2). Recommendation: keep it. It is the
   only thing that makes "did not say" mean one thing in the document,
   and `CLAUDE.md`'s *no compatibility to preserve* removes the usual
   objection. Fallback: revert to 1, one constant and three test strings,
   and accept that an absent pack figure is two statements at once.
2. **The write outside *Owns*** (*Risks* 1). Recommendation: accept it
   and tell P6, since P6 also owns `status.rs` and must rebase on this
   branch rather than beside it. Fallback: none that leaves the suite
   green.

## Resume point

```sh
# P3 (the runner's gc) and P4 (custody) depend on P1, not on this branch.
# P6 (no holder is a named state) owns herd-hub/src/registry.rs,
# server.rs and status.rs, and must branch off the batch that lands this
# one, not off b99d32d: it wants the same RunnerNow literal at
# server.rs:2112 and the same RunnerNow struct in status.rs.
git worktree add -b m12-p6-no-holder <path> origin/main   # after landing
# What P6 inherits, and should not re-derive:
#   - status.rs's STATUS_VERSION is 2; a fourth field moves it to 3 only
#     if absence would otherwise become ambiguous. The reason test is on
#     the constant.
#   - a figure a runner did not give prints `did not say`, never
#     `unknown` and never 0 (status.rs DID_NOT_SAY, and the module
#     header's two-words paragraph).
#   - registry.rs's MANIFESTS_DIR is private and report.rs spells it a
#     second time under a guard. P6 owns registry.rs: making it
#     pub(crate) and having report.rs use it removes the duplication,
#     and the guard test then asserts the same thing for free.
#
# ORCH-256's remaining half, for whoever gets it: the runner's
# packs/refused/ in a report, on herd-runner. D233 keeps it
# indefinitely and ORCH-215 owns whether it may ever be removed.
```

## Processes

- **Started and ended by this agent:** `cargo`/`rustc` under
  `nix develop` — `build --workspace` (three times), `fmt --all` and
  `fmt --all --check`, `clippy --workspace --all-targets`, two full
  `./scripts/test.sh` runs and about a dozen narrowed `cargo test` runs,
  each in the foreground or detached and each exited before the next; one
  `herd-hub` binary run by hand, three times, against hand-made state
  roots in this agent's scratchpad, now deleted. **Six `herd-runner` and
  two `herd-hub` processes** were spawned by `herd-hub/tests/
  pack_status.rs` itself under `kill_on_drop`, on a loopback port 0, with
  their roots in cargo's temporary directories; each was killed by its
  own test. **No PID signalled by hand, nothing killed by name, no
  `pkill`.**
- **Not touched:** the consumer's hub `3006543` and runner `3021002`,
  and this project's own pinned v0.4.1 instance (hub `565152`, runner
  `607643` on `127.0.0.1:8791`). Nothing on `/etc/nixos`, `/srv`, a
  container or a service.
- **`./scripts/bench-preflight.sh` before each heavy phase**, and it
  **refused once and was obeyed**: `load 15.16 is at or above 9.00 on 12
  cores`, before the second suite run. The phase was not started; the
  agent waited on the condition and the next clearance came at load
  **4.86**. Five clearances in all, at loads 0.73, 0.96, 1.96, 3.41 and
  4.86, and one refusal at 15.16. No clause lifted,
  `HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB` never set.
- **Build tree:** `/mnt/ssd_980/herd/build/m12-p2b`, deleted by literal
  path once the branch is pushed; the figure with its profile is in the
  agent's report. Never on the root filesystem; the worktree's own
  `target/` holds only `scripts/test.sh`'s three small log files.
