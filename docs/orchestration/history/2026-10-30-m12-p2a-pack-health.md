# m12-p2a-pack-health — handoff 2026-10-30

- **Item:** ORCH-251, **P2a's half of it only**. The row covers P2a then
  P2b; P2a is done and the row is amended rather than closed, because
  the hub still discards all three fields and that is P2b.
- **Agent:** herd-step
- **Branch / commit:** `m12-p2a-pack-health` / `1d645d4` (this handoff's
  commit follows it), based on origin/main `a8cf397`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

**`herd-runner/src/lib.rs`** (+356 / −7)

1. **The three figures stop being `None` at the build site.**
   `ToHub::Health`'s `pack_store_bytes`, `pack_store_budget_bytes` and
   `packs_held` are read by one new function,
   `what_the_pack_store_says(&Inner)`, called from inside
   `queue_a_health_report`'s own critical section — the same section
   that queues the frame and counts it, so nothing about E-40's
   `reports_sent` invariant moved.

2. **Absence keeps its meaning, and it means three different things.**
   The function has three answers and the doc comment says which is
   which:

   | state | bytes | budget | packs |
   |---|---|---|---|
   | no store opened yet | absent | absent | absent |
   | store open, nothing measured it | absent | **as given** | absent |
   | measured, no `--pack-budget` | the figure | absent | the figure |

   The second row is the one `PackUse::measured` exists for: *false is
   not nought*. The budget travels anyway, because a budget is a figure
   its owner typed and not one this runner read.

3. **One route to the store, which is P1's finding held open rather
   than re-derived.** `Session::pack_store(&self, data_root)` is now the
   **only** caller of `PackStore::open` in the runner crate; it opens
   and remembers the handle on `Inner::pack_store`. Its one caller is
   `announce_packs_held`, which both of its own callers hand
   `confined.root().path()` — the path `DataRoot::open` canonicalised
   (`root.rs:140`). The report then reads that handle. **There is no
   second path for the report to get wrong**, so the defect ORCH-251's
   Item cell describes has nowhere to come back from in this file.

4. **No filesystem access on the report.** `usage()` and `budget()` are
   figures the store keeps (P1's `Standing`), which is what lets this
   run on the cadence and under a lock. The counter P1 built for exactly
   this claim — `objects_listed` — is untouched and its test
   (`saying_what_this_runner_holds_lists_no_objects_directory`) still
   holds; `PackStore::open`'s two `create_dir_all`s are still paid once
   per announcement and **not** once per report.

5. **The count narrows honestly.** `PackUse::packs` is `u64` and the
   wire is `u32`: `u32::try_from(..).ok()`, so a count that does not fit
   is sent **absent** and never saturated. A figure this runner cannot
   say is one it does not say.

**Four tests in the crate's own module** (`herd-runner/src/lib.rs`,
`mod tests`), with two fixtures beside them — `plant_a_pack_store`,
which plants a store in the shape a runner's *second life* finds it in,
and `a_session_on_a_link_announcing`, which reproduces the dial's own
order: attach, announce, report.

| test | what it holds |
|---|---|
| `a_health_report_carries_the_pack_store_figures_and_its_budget` | the fixture's own summed object lengths (4608 from 11, 500 and 4097), its pack count (3) and the typed budget (7 000 000 003) — none absent, none nought |
| `a_runner_with_no_pack_budget_still_reports_what_it_holds` | the plan's own clause: budget `None`, the other two real |
| `a_runner_that_has_not_read_its_pack_store_says_nothing_rather_than_nought` | **both** shapes of "did not say": no store opened at all is three absences; a store open and unmeasured is two absences and a budget that still travels |
| `the_report_reads_the_store_the_data_root_was_validated_at` | a real root and a symlinked spelling of it: the budget taken under the typed spelling is the budget reported under the canonical one, and the report's figures are asserted to **agree** with a store opened again on the data root's own path, not assumed to |

**Two tests against the real binaries**, in a new file
`herd-hub/tests/pack_store_health.rs` (+301):
`a_runner_holding_a_known_store_reports_all_three_figures` and
`a_runner_with_no_pack_budget_still_says_what_it_holds`. A real
`herd-runner` process, a planted store on its disk, a hand-written hub
that welcomes it and decodes the first `ToHub::Health` off the socket.
Both read the frame rather than a sentence about it.

**The frame, read off the real wire** (a temporary `eprintln!` in the
test's hub, run under `--nocapture`, then removed and the file restored
to a byte-identical sha256 — `9d35cd29…`, verified with `sha256sum -c`):

```
{"type":"health","jobs":[],"draining":false,"load_milli":1710,
 "switch":{"position":"on"},"pack_store_bytes":4608,
 "pack_store_budget_bytes":7000000003,"packs_held":3}
{"type":"health","jobs":[],"draining":false,"load_milli":1710,
 "switch":{"position":"on"},"pack_store_bytes":4608,"packs_held":3}
```

(Line-wrapped here; one line each on the wire.) The second is the
no-budget runner: **the key is absent from the JSON**, not `null` — the
`skip_serializing_if` doing at the wire what the `Option` means in the
type. ORCH-186's lesson was followed and found nothing to correct:
this task renders no new sentence, the figures travel as numbers, and
the one rendered line near them is P1's startup sentence, unchanged and
asserted in the new test.

**Each new assertion was demonstrated red before it was trusted**, four
breaks, the file restored to a byte-identical sha256 each time
(`sha256sum -c`, three files checked after every restore):

- `what_the_pack_store_says` forced to `(None, None, None)` — the
  pre-P2a state — reddens **all four** unit tests
  (`left: (None, None, None)`, `right: (Some(4608), Some(7000000003),
  Some(3))` and the three others) **and both real-binary tests**
  (`left: None, right: Some(4608)`, `pack_store_health.rs:249` and
  `:292`);
- the unmeasured branch returning `(None, None, None)` — dropping the
  budget with the measurements — reddens the "did not say" test **alone**
  (`left: (None, None, None)`, `right: (None, Some(7000000003), None)`),
  the other three green;
- the same branch returning `(Some(0), budget, Some(0))` — nought
  instead of silence — reddens the same test alone
  (`left: (Some(0), Some(7000000003), Some(0))`);
- `packstore`'s `keyed(path)` replaced by `path.to_path_buf()`, which is
  P1's own break re-applied: the spelling test reddens
  (`left: (None, None, None)`, `right: (Some(4608), Some(7000000003),
  Some(3))`) together with P1's
  `two_spellings_of_one_data_root_are_one_store`, and the other three
  unit tests stay green — they run on an already-canonical `TMPDIR`.
  `herd-runner/src/packstore.rs` was restored to `73b49c16…`, the sha
  P1's handoff records, and is **not** in this branch's diff
  (`git status` names two files only).

**`docs/orchestration/board.md`**: ORCH-251's *Next action* amended —
P2a done with its commit, its route, the wire reading, and **what is
left of the row is P2b alone**; State stays `owed` and nothing is
closed. Its *Evidence* gains the two new sites and this file.

## Not done, and why

1. **The hub still discards all three fields.** `herd-hub/src/server.rs`
   binds and drops them in its `ToHub::Health` arm (at `:4871-4873` on
   this base — ORCH-251 quotes `:4692-4694` from an older tree). That is
   **P2b**, whose *Owns* is the hub's `server.rs`, `status.rs` and
   `report.rs`; this task's *Owns* is `herd-runner/src/lib.rs`. So
   nothing an operator can read has changed yet: the figure is on the
   wire and in nobody's `status.json`.
2. **`docs/status.md` was not touched**, and nothing here makes a
   sentence of it false. Its item 1 says the runner takes a budget,
   evicts nothing and the deployed unit names none — all three still
   true, and none of them is about reporting.
3. **No `docs/decisions.md` entry.** The brief asked for none and
   nothing here is a decision rather than the plan's instruction carried
   out; `decisions.md` is `herd-merger`'s alone. Nothing drafted either
   — see *Drafted for `herd-merger`*.
4. **No eviction, no keeper, no locality.** P3, P4 and Track L.

## Gates

**Run before this commit, on the code commit `1d645d4`'s tree**, all
four Rust gates under `nix develop --command`, with
`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/m12-p2a`:

```
=== cargo build --workspace
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 26.06s
BUILD EXIT: 0
=== cargo fmt --all --check            (bare, output empty)
FMT EXIT: 0
=== cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 25.97s
CLIPPY EXIT: 0
```

`clippy --all-targets` is also the answer to this branch's own exposure
to **ORCH-235**: the new test file constructs `FromHub::Welcome` with
all five of its fields, and every test crate compiles here.

```
=== nix develop --command ./scripts/test.sh
SUITE EXIT: 0
ignored census: COMPLETE (38 ignored, 0 without a reason)
test suite: PASSED
summed from the per-target lines: 1154 passed, 0 failed, 38 ignored
skips: 14
```

**The 14 skips, by name** (D187: a list, never a count). Twelve are the
standing cgroup-delegation set, unchanged:
`a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device`,
`a_declared_ceiling_changes_what_the_inert_weight_line_says`,
`a_job_cannot_fork_past_its_own_pid_ceiling`,
`a_job_s_cgroup_carries_the_cpu_io_and_pid_bounds`,
`a_job_s_cgroup_carries_the_hard_io_ceiling_it_was_given`,
`a_job_s_record_carries_the_pressure_it_ran_under`,
`an_allocation_past_the_ceiling_kills_the_job_and_only_the_job`,
`an_io_weight_that_cannot_bite_is_said_out_loud`,
`a_tighter_enclosing_memory_max_lowers_the_room_that_is_announced`,
`no_declared_ceiling_leaves_io_max_empty`,
`no_declared_quota_leaves_cpu_max_at_the_kernel_s_max`,
`the_runner_says_which_memory_ceiling_it_enforces`.
Two are the load-gated flood benches, which **declined as a delta**:
`flood-fifty` and `flood-two-fifty`, both *"the host's one-minute load is
Some(9.92), over this bench's ceiling of 8; the host is shared with a
production system"*. That is the host and not the diff, and nothing was
done about it. **None of the 14 is a pack skip and none is new.** D27:
printed as skipped, never counted as passed.

**The last run, named and not quoted:** `./scripts/check-docs.sh`, run
bare on the tree this handoff is committed into, its output to
`p2a/check-docs-last.log` in this agent's scratchpad (the full path is in
the agent's report) and verbatim in that report. `./scripts/gate.sh` was
**not** run: the batch's gate is `herd-merger`'s.

## Risks and findings

1. **P1's predicted symptom is not the symptom, and the real one is
   quieter.** P1 and ORCH-251 both say a figure keyed on the literal path
   would publish *"no budget, 0 bytes"*. Measured here by re-applying
   P1's own break: the report goes to **all three absent** — not `0` —
   because the second `Standing` has `measured: false`, and this task
   sends nothing for an unmeasured store. So the failure mode is a runner
   holding gigabytes that **says nothing at all**, which is strictly
   better than lying and still useless to an operator, and it would be
   just as invisible on a machine nobody here may log into (D161).
   Worth correcting in the row's wording, which this branch does.
2. **`announce_packs_held` now has a side effect, and it is deliberate.**
   It is documented at both ends (`Inner::pack_store` and
   `Session::pack_store`). The alternative — remembering the store at the
   dial site beside the announcement — is two places that must agree
   about one path, which is the shape of the defect this is guarding
   against. The cost: **a runner whose store will not open reports
   nothing about packs**, for ever on that connection, and says so once
   on its stderr in the announcement's own sentence. That is right
   (an error is not an emptiness) and it is a silence an operator can
   only see in the runner's log, not in the hub's.
3. **The report is only as fresh as the announcement's opening.**
   `PackStore::open` is still called once per announcement and the
   figures live in the shared `Standing`, so a report after an ingest
   sees the ingest. But if a future change stops announcing on a
   connection, the handle goes stale as a *handle* and the figures stay
   correct anyway — they are the data root's, not the value's. Nothing to
   do; stated because the coupling is new.
4. **P1's finding 3 is unchanged and now slightly cheaper to care
   about.** `PackStore::open`'s two `create_dir_all`s are paid per
   announcement, not per health report: the report never opens anything.
   If P3 wants the announcement cheaper, that is still its own call.
5. **A pipe hid a gate's status, again, on this branch.**
   `cargo fmt --all --check 2>&1 | head -40` printed four real rustfmt
   diffs and `FMT EXIT: 0` from `${PIPESTATUS[0]}` — `head` closing the
   pipe is enough to lose it. The gate was then run **bare**, to a file,
   and was genuinely red until `cargo fmt --all` fixed it. Every gate
   quoted above was run bare to a file. This is the fifth sighting of
   the shape the board already carries; no new row, because it is the
   same row's measurement.
6. **The new test file is ORCH-235's exposure, named.** It constructs
   `FromHub::Welcome` with `protocol_version`, `hub_version`,
   `state_root`, `version_note` and `keeper_for` — complete **about this
   base, `a8cf397`**. A branch that widens `Welcome` and lands beside
   this one gives `E0063` here, not a wrong answer. R1
   (`m12-r1-free-space-guard`) touches `herd-hub/src/job.rs`,
   `store.rs`, `main.rs`, `herd-hub/tests/retention_preview.rs` and
   `herdctl/tests/fixture/mod.rs`: measured with
   `git diff --stat origin/main origin/m12-r1-free-space-guard`, **no
   overlap** with either of this branch's two files, and nothing here
   constructs the `Budget` it widened.
7. **`herd-hub/tests/pack_store_health.rs` is outside this task's
   stated *Owns*, which is `herd-runner/src/lib.rs`.** It is a new file,
   so it claims nothing anybody else holds, and it is beside the hub's
   tests because `no_module_of_the_runner_can_bind_a_socket`
   (`herd-runner/tests/confinement.rs`) forbids the string `TcpListener`
   anywhere under the runner crate, tests included — invariant 3. That is
   `fake_hub_flood.rs`'s own reasoning, quoted in the file's module doc.
   P2b owns `herd-hub/src/`, not `herd-hub/tests/`, so the two do not
   collide; if P2b wants to assert the hub's side of these fields it
   should add its own file rather than edit this one.
8. **The hand-written hub answers nothing and the runner does not mind.**
   It welcomes, then reads. It never sends `HealthRecorded`, which the
   runner's `reports_recorded` counter would otherwise see — nothing in
   these two tests depends on that counter, and the test ends after one
   report. A later test that needs several reports on one link should
   answer them.

## Drafted for `herd-merger`

**No `docs/decisions.md` entry.** The brief asked for none, and nothing
here is a decision: the shape of the three absences is
`herd_proto::ToHub::Health`'s own documented promise, carried out.

**No new `board.md` row.** ORCH-251 is the row for this work and it is
**amended on this branch**, not closed: its *Next action* now records
P2a done with its commit and its wire reading and says the row stays
open for P2b alone, and its *Evidence* names the two new sites and this
handoff. The merger has nothing to paste.

Two corrections the merger may want to carry into its own record, both
measured here and neither a row:

1. ORCH-251's Item cell cites the hub's discard at
   `herd-hub/src/server.rs:4692-4694`; on `a8cf397` it is **`:4871-4873`**.
   The line numbers moved with batch 19; the arm is the same one.
2. ORCH-251's Item cell predicts *"no budget, 0 bytes"* as the symptom of
   the canonical-path defect. Measured, it is **all three absent** — see
   *Risks and findings* 1. The row's amended Next action says so.

## Decisions needed

**None.** One thing for the orchestrator's eye and it is not a question:
**the figure now leaves the runner and still reaches nobody.** The hub
discards all three fields, so until P2b lands, this task's whole value is
a frame a test reads. P2b is one task, its *Owns* collides with nothing
currently pushed, and ORCH-251 is already pointed at it.

## Resume point

```sh
# P2b, in its own worktree off the batch that lands this one:
git worktree add -b m12-p2b-pack-status <path> origin/main
# it owns herd-hub/src/server.rs, status.rs and report.rs. The three
# fields arrive now — read the frame this branch's test reads:
#   {"pack_store_bytes":4608,"pack_store_budget_bytes":7000000003,
#    "packs_held":3}
# and, for a runner that said nothing, the KEY IS ABSENT, not null:
#   herd-hub status must print "did not say" and never 0, per field,
#   because the three absences do not mean the same thing (this
#   handoff's Done 2). STATUS_VERSION moves if the document's shape
#   does. `herd-hub report` counts packs/manifests/ on the hub side.
# The arm to stop discarding is herd-hub/src/server.rs:4871-4873 at
# a8cf397 — ORCH-251's :4692-4694 is an older tree's numbering.
```

P3 (the runner's gc) and P4 (custody) depend on P1, not on this branch,
and need nothing from it.

## Processes

- **Started and ended by this agent:** `cargo`/`rustc` under
  `nix develop` for build, `build --all-targets`, fmt, clippy, one full
  suite run and six narrowed `cargo test` runs, each in the foreground
  and each exited before the next. Four `herd-runner` processes, spawned
  by `herd-hub/tests/pack_store_health.rs` itself under `kill_on_drop`
  against a loopback listener on port 0, with data roots in cargo's
  temporary directories; each was killed by its own test. **No PID
  signalled by hand, nothing killed by name, no `pkill`.**
- **Not touched:** the consumer's hub `3006543` and runner `3021002`,
  and this project's own pinned v0.4.1 instance (hub `565152`, runner
  `607643` on `127.0.0.1:8791`).
- `./scripts/bench-preflight.sh` run before each heavy phase: two
  clearances, `ok, safe to start` each time, at loads **1.88** and
  **6.10** on 12 cores against a ceiling of 9.00. No clause lifted,
  `HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB` never set.
- **Build tree:** `/mnt/ssd_980/herd/build/m12-p2a`, **14G** by
  `du -sh`, all of it the **debug** profile
  (`/mnt/ssd_980/herd/build/m12-p2a/debug`, 14G; the only other entry is
  a 0-byte `tmp/` and `CACHEDIR.TAG`). Deleted by literal path once the
  branch is pushed; never on the root filesystem. The worktree's own
  `target/` was never created.
