# fix-four-false-sentences — handoff 2026-10-31

- **Item:** ORCH-304 (corrections 1 and 2) and ORCH-282 (correction 4);
  correction 3, `herd-runner/src/packstore.rs`, has no board row
- **Agent:** herd-step
- **Branch / commit:** `fix-four-false-sentences` / `cbf560a5ade46ab6`
  (the gated sha; this handoff and the board are the commit after it),
  based on `origin/main` `ae3ae40c7410ee9086dda5f40d66e446b5cfc383`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

Four sentences that `ae3ae40` refutes are replaced by readings of the
code, each naming the line that decides it. **No behaviour moved:** the
diff is four doc comments and one prose section, and `git diff
--stat` for the code commit is `workflow.md` 51, `job.rs` 26,
`registry.rs` 25, `packstore.rs` 24 — 105 insertions, 21 deletions, no
statement touched.

Each claim was read against `ae3ae40` before it was edited. **All four
were false**, three of them wholly and one of them in one of its two
clauses; the true clause of that one survives in the replacement.

### 1. `herd-hub/src/registry.rs:19`-`:23` — false in its *layout* half

The sentence read *"What is still owed on that side is **the recording
and the layout**: the resolved ids are not written to
`JobRecord::packs_in` …, and a runner handed a pack hands the attempt
back rather than laying `{in}/<key>` out, because the layout step is not
built."*

- **The recording half is true and is kept.** `packs_in` is written by
  nothing: the four `JobRecord` constructions under `herd-hub/src`
  (`queue.rs`, `job.rs`, `matcher.rs`, `store.rs`) all set it to
  `Default::default()`, and the only other `packs_in` in the crate is
  `server.rs`'s `let packs_in = match packs_for(state, record)`, which
  goes onto `FromHub::AssignJob` eleven lines later — the **frame's**
  field, not the record's.
- **The layout half is false, and the deciding line is
  `herd-runner/src/job.rs`'s `Packs::why_this_runner_cannot`**, whose
  first statement is `if declared || (self.packs_in.is_empty() &&
  self.packs_out.is_empty()) { return None; }`. A declared command is
  never handed back for its packs; its keys reach
  `Session::lay_out_packs` (`herd-runner/src/lib.rs`), called from
  `herd-runner/src/job.rs`. What is handed back is an **undeclared** job
  that asks for or leaves a pack, which has no `{in}` to lay one out in.
  D267 landed both halves of the layout in batch 26, and
  `herd-runner/src/lib.rs` already carried the honest wording: *"the
  layout step is landed — `Session::lay_out_packs` is what a running
  job's `{in}` comes from"*.

The replacement widens the surviving clause, because `packs_out` is in
the same state (next item), and states the layout as landed with the
predicate that decides the hand-back.

### 2. `herd-hub/src/job.rs:1133`-`:1135` — false

`JobRecord::packs_out`'s doc read *"Written when a runner's post-fsync
registration frame is accepted, so a key appears here only once the pack
exists somewhere whole."* Nothing writes it.

- **The deciding line is the arm itself**, `ToHub::RegisterPack` at
  `herd-hub/src/server.rs:6221`: it opens the registry, checks the
  attempt may register, builds a `registry::Registration`, calls
  `registry.register(...)`, journals the result, sends
  `FromHub::PackRegistered` and calls `tell_the_keepers`. It never
  touches a record.
- Every `JobRecord` built under `herd-hub/src` sets `packs_out` to
  `Default::default()` and no later line assigns it. `server.rs:5637`'s
  `packs_out: record.leaves.clone()` is the **assignment frame's** field
  and says what a job was *asked* to leave.
- The field now reads the way `packs_in`'s honest doc two fields above
  already did (*"Written by nothing on this build, and that is a
  measured gap and not the design"*), with the intended meaning marked
  **To be written**.
- **One clause of the old sentence was false in a second way** and is
  not carried over: it said the reason for an absent key *"is in
  `events.jsonl`"*. `journal` (`herd-hub/src/server.rs:6646`) is
  `writeln!(std::io::stderr().lock(), …)` — the hub's stderr, not a
  job's event file. The replacement names the hub's journal and
  `packs/manifests/<id>/registered.json`, whose `RegisteredFile` holds
  `job` and `attempt` *"so a pack's provenance is on the disk and not
  only in a log line"*.

### 3. `herd-runner/src/packstore.rs:546`-`:548` — false

`Claims::pins`' doc read *"what makes a hub **name** a keeper is M12's
keeper policy, which is the hub's and is not written yet"*. It is
written, and landed in batch 25 (D266).

- `herd-hub/src/server.rs:345` is `Keepers`, *"Who keeps which packs —
  the hub's custody policy (D10, M12, E-16)"*, and `herd-hub/src/main.rs`
  carries `--keeper RUNNER` (`:269`) and `--keeper-for KEY=RUNNER`
  (`:291`).
- Every welcome is filled from it: `server.rs:3659`, `keeper_for:
  custody.clone()` inside the `FromHub::Welcome` that is sent on each
  connection. `tell_the_keepers` sends `PinPacks` when a registration
  moves a set.
- **The clause before it was false too, in the same sentence**, and is
  corrected with it: *"`None` … is still every runner a hub of this
  build talks to"*. It is now every runner of a hub **nobody
  configured**, and the line that decides it is that neither option has
  a default — `Keepers`' own doc says *"a hub started with neither
  setting names nobody, sends `keeper_for` on no welcome and sends no
  `PinPacks` at all"*. That is the consumer's hub, so `pins: None` is
  still the ordinary case; it is no longer an unbuilt half.

### 4. ORCH-282, `docs/orchestration/workflow.md` — false

The *Execution goes through Herd* bullet read *"a job gets **no
workspace** — it `cd`s into the requester's worktree, so `gate.sh`'s
receipt lands in `<worktree>/target/herd-gate/` even when
`CARGO_TARGET_DIR` points elsewhere"*. A job does get a workspace, and
it is not the requester's tree.

- `herd-runner/src/root.rs` creates `work`, `home` and `tmp` under the
  job's base, after reclaiming any leftover *"so the workspace starts
  empty"*, and `Workspace::work_dir` is *"the job's working directory,
  and the only one it is told about"*.
- **The deciding line is `herd-runner/src/job.rs:442`**,
  `.current_dir(workspace.work_dir())`: that is where a step is spawned,
  and nothing in the runner enters a worktree.
- Measured in job `j0000000063` on 2026-10-30
  (`history/2026-10-30-orch-273-prove-io-max.md`, finding 4): the cwd is
  `/mnt/ssd_980/herd/queue/runner-data/jobs/<id>/work`, empty and not a
  git repository, with a per-job `HOME` and no `cargo` on `PATH`.
- The replacement says plainly that the `cd` is the requester's own and
  goes inside the submitted command, by absolute path, and carries a
  worked `bash -c` submission so nobody rediscovers it.
- **The bullet's receipt clause was false as an explanation, and that was
  not in the row.** `gate.sh` takes its root from its own location, not
  from the cwd: `root=$(cd "$(dirname "$0")/.." && pwd)`, then
  `receipts=${HERD_GATE_RECEIPTS:-$root/target/herd-gate}`. So the
  receipt lands in the worktree whose `scripts/gate.sh` was invoked
  whatever the cwd is — which is **demonstrated by this task's own gate
  run**: the receipt is under this worktree's `target/herd-gate/` while
  `CARGO_TARGET_DIR` was `/mnt/ssd_980/herd/build/orch-308`.

### Added to the same section, as the brief asked

Only what was missing. `--hub ws://127.0.0.1:8791` (bullet 1, via the
wrapper), `--label build` and `--memory-mib` (bullet 2),
`./scripts/bench-preflight.sh` first (bullet 4) and `--verdict-to`
(bullet 5) were already there and were left alone. New:

- the worked `bash -c "cd <absolute worktree> && …"` submission;
- **a distinct `CARGO_TARGET_DIR` per agent, written into the command**,
  with the reason: E-05's deduplication key is over the command, the
  label set and a declaration's commit and nothing else — not the
  requester, not the time, not the priority — each piece
  length-prefixed (`herd-hub/src/job.rs`, `NewJob::dedup_key` and
  `dedup_key`), so two identical submissions are **one job** and the
  second **joins** the first.

**A word of the brief was changed on purpose:** the brief says two
identical submissions are *coalesced*. In this tree *coalesce* is
reserved for **discarding** and *dedup / join* for this, which is
ORCH-299's own distinction (*"dedup — joining — is on `main` and
asserted …, coalesce — discarding — exists now"*), so the section says
*joins*.

### Board

`ORCH-282` and `ORCH-304` move `owed` → `done` with a dated clause in
*Next action* naming the branch, what the replacement says, and in each
case the premise of the row that the reading corrected. No row is
added: the ids are the merger's.
## Not done, and why

- **`docs/decisions.md`: untouched.** Only `herd-merger` appends
  (invariant 8). Nothing is drafted for it either: four corrections of
  false prose decide nothing, and each already has its decision —
  D266 for the keeper policy, D267 for the layout, D258 and ORCH-282 for
  the job workspace.
- **`docs/coverage.md`: untouched.** Another agent holds it for
  ORCH-305/306/307, as the brief says.
- **`deploy/release-notes/1.0.0.md`: untouched.** Published.
- **`herd-hub/tests/pack_custody.rs:14`-`:16` is a fifth instance of
  correction 3's defect and is left standing**, because it is outside
  the paths this brief allows. A row for it is drafted below.
- **`CLAUDE.md`'s *Commands* block does not carry the `cd` either**, and
  it is outside the brief's paths. A one-clause insertion is drafted
  below for whoever owns that file next.
- No test was added. There is nothing to assert: the change is four
  comments and one prose section, and a test that read a doc comment
  would freeze the wording rather than the fact.

## Gates

**All four Rust gates ran, through Herd, on the gated sha**
`cbf560a5ade46ab63ce2afa3cb16d610f8add003`. `./scripts/gate.sh` steps 3
to 6 are `nix flake check --no-build`, `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets -- -D warnings` and
`./scripts/test.sh`; steps 1 and 2 are `check-docs.sh` and
`check-orchestration.sh`. **No gate was run on this host directly.**

- **Preflight:** `./scripts/bench-preflight.sh` →
  `bench-preflight: ok, safe to start.` (load 6.25 of a 9.00 ceiling,
  slice room 15001 MiB over a 8192 MiB floor).
- **Submission:** `/mnt/ssd_980/herd/queue/bin/herdctl --hub
  ws://127.0.0.1:8791 run --label build --memory-mib 4700 --verdict-to
  <file> -- bash -c "cd <worktree> && export
  CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/orch-308 &&
  ./scripts/gate.sh"`.
- **Herd job `j0000000171`**, `"passed": true`, `"exit_code": 0`,
  `ran on: queue-local` — the whole verdict document is in the agent's
  report.
- **Gate's own last line, verbatim:**

  ```text
  gate: GREEN sha=cbf560a5ade46ab63ce2afa3cb16d610f8add003 steps=0-8:ok host=servernixos load1=4.82 load1-end=9.60 skips=under-load(flood-fifty=67.8>8,flood-two-fifty=67.8>8)
  ```

- **Receipt:** `<worktree>/target/herd-gate/<sha>.receipt`, the sha above
  — `verdict: GREEN`, `steps: 0..8 all ok`, `skips-observed: 14`,
  `ignored-observed: 38`, started `2026-10-31T00:08:08Z`, finished
  `2026-10-31T00:19:04Z`. The absolute path is in the agent's report.

**Skips — 14, each named, none counted as a pass (D27).** Step 7 found
`the skip list matches the expectation exactly` against
`scripts/expected-skips/local.txt`, and the two that are not on the
at-rest list were excused by name out of
`scripts/expected-skips/local-under-load.txt`:

- **Twelve for the same cause, no cgroup v2 delegation on this host**
  (`Permission denied` creating a cgroup under this scope, D23's
  `RLIMIT_AS` fallback):
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
- **Two load-gated benches that read the host and declined**:
  `flood-fifty` and `flood-two-fifty`, both
  `the host's one-minute load is Some(67.8), over this bench's ceiling
  of 8`. Excused by the delta list, by name and by reason.
- **38 `#[ignore]`s**, list unchanged and every one with a reason:
  `ignored census: COMPLETE (38 ignored, 0 without a reason)`.

**The last run, named and not quoted** — on the committed tree, after
this file: `./scripts/check-docs.sh` and
`./scripts/check-orchestration.sh`, run in the worktree, their output in
the agent's report.

## Risks and findings

1. **A fifth instance of correction 3's false sentence is on `main`**,
   in a test's module doc: `herd-hub/tests/pack_custody.rs:14`-`:16`
   says *"The real hub sends `keeper_for: Vec::new()` at
   `herd-hub/src/server.rs` and sends no `FromHub::PinPacks` at all:
   choosing a keeper is M12's keeper policy, which is the hub's half and
   is not written yet."* Both clauses are false since batch 25 (D266):
   `server.rs:3659` sends `keeper_for: custody.clone()` and
   `tell_the_keepers` sends `PinPacks`. The test's *conclusion* is still
   right — a hand-written hub is the honest observation point, because
   the shipped hub names a keeper only when an operator passes
   `--keeper` or `--keeper-for` — so this is a false reason under a
   sound decision, which is the worst kind to leave: the next reader
   trusts the reason. Row drafted below.
2. **Two neighbouring comments in `packstore.rs` read as though the
   layout were unbuilt, and are defensible as written.**
   `herd-runner/src/packstore.rs:82`-`:85` (*"what a queued or running
   job references is knowledge the layout step has not put in this
   runner yet"*) and `:524`-`:529` (*"the layout step that puts it there
   is M11's (ORCH-220)"*) are both true of what they actually claim —
   `Session::lay_out_packs` does not write what it laid out into the
   session, which `herd-runner/src/lib.rs` states exactly — but a reader
   who stops at either will conclude the layout is unbuilt. Not edited:
   they are not false, and the brief's instruction was to leave a claim
   that is not false alone. Worth one sentence from whoever next opens
   that file.
3. **The board row ORCH-282 carried a false explanation of its own**,
   and the fix does not inherit it: the receipt does not land under the
   worktree because of the `cd`. It lands there because `gate.sh`
   resolves `root` from `$0`. The row's *Next action* cell now says so.
   This is the second time in two days that a row was right about a
   defect and wrong about its mechanism (D278's case was the release
   body quoting `coverage.md`); the cost here would have been a reader
   who moved the `cd` and expected the receipt to move with it.
4. **The gate's verdict covers `.rs` and prose both, but the board row
   edit was made after the job's step 2 had finished.** The gate's step
   0 read a clean tree at `cbf560a` and its steps 1 and 2 (`check-docs`,
   `check-orchestration`) ran at 00:08 UTC, before `board.md` was
   touched; the suite then ran on a tree carrying that one prose edit.
   No `.rs` file changed after submission. The two docs checks are
   therefore re-run on the final committed tree and quoted in the
   agent's report, which is what makes the prose half green.
5. **`j0000000171`'s own resource figures are not evidence of anything**
   on this host: our runner has no cgroup, so a record's
   `peak_memory_bytes` is what the runner could see (ORCH-268, and
   `workflow.md` *What the rule does not buy*). The `--memory-mib 4700`
   is an admission figure, not a bound.

## Drafted for `herd-merger`

**For `docs/decisions.md`:** none. Four corrections of false prose
decide nothing new; each states a decision that is already in the file
(D266, D267, D258).

**For `docs/orchestration/board.md`** — one new row, in *Owed*, after
ORCH-307. The highest id on `main` is **ORCH-307**:

```markdown
| ORCH-NEXT | **A test's module doc gives a false reason for a sound decision: `pack_custody.rs` says the hub's keeper policy is not written.** `herd-hub/tests/pack_custody.rs:14`-`:16` reads *"The real hub sends `keeper_for: Vec::new()` at `herd-hub/src/server.rs` and sends no `FromHub::PinPacks` at all: choosing a keeper is M12's keeper policy, which is the hub's half and is not written yet."* Both clauses are false since batch 25 (**D266**): `herd-hub/src/server.rs:3659` sends `keeper_for: custody.clone()` on every welcome out of `Keepers` (`:345`, from `--keeper` and `--keeper-for` at `herd-hub/src/main.rs:269` and `:291`), and `tell_the_keepers` sends `PinPacks` to the runners whose set moves. The test's **conclusion** is still right, which is why this is a row and not a red test: a hand-written hub is the honest observation point because the shipped hub names a keeper only when an operator passes one of those two options, neither of which has a default. So a false reason sits under a sound decision, and the next reader trusts the reason | `fix-four-false-sentences`, 2026-10-31, *Risks and findings* 1, found while correcting the same sentence in `herd-runner/src/packstore.rs:546`-`:548`; outside that task's allowed paths | herd-step | owed | Rewrite the *Why the hub here is hand-written* paragraph to the measured reason: the shipped hub **can** make a runner a keeper, and does so only when an operator asks, so a test that wanted one from a real hub would be testing the policy and not the custody. Check `pack_store_health.rs` beside it, which the paragraph says shares the reasoning. **Prose inside a test file, so it owes the four Rust gates** — `nix flake check`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `./scripts/test.sh`, each under `nix develop --command` or through one `./scripts/gate.sh` submitted to Herd | — | `herd-hub/tests/pack_custody.rs:14`-`:16`; `herd-hub/src/server.rs:345` `Keepers` and `:3659`; `herd-hub/src/main.rs:269` and `:291`; `herd-runner/src/packstore.rs` `Claims::pins`; **D266**; ORCH-283, ORCH-304 |
```

**For `CLAUDE.md`'s *Commands* block** — not applied, because the file
is outside this brief's paths and has a line budget
`check-docs.sh` enforces. The clause that is missing there, in the
smallest form that carries it:

```markdown
A submitted command starts with its own absolute `cd` — a job's cwd is an
empty `<data-root>/jobs/<id>/work` — and carries a `CARGO_TARGET_DIR` of
its own, or the hub joins it to another agent's identical job (E-05).
```

## Decisions needed

None. Everything here is a reading of the tree at `ae3ae40`, and the one
thing that could have been a question — whether the brief's *coalesced*
or the tree's *joins* is the right word — is settled by ORCH-299 in the
tree and is recorded above rather than asked.

## Resume point

Nothing is in flight. The next agent in this area takes the drafted row
above (`pack_custody.rs`'s paragraph), or ORCH-283, which is the open
question about reloading the keeper policy on a running hub.

## Processes

- Herd job `j0000000171`, submitted by this task and finished; no
  process of the runner or the hub was signalled. The only PIDs this
  task started were its own `herdctl` client and its shell helpers, all
  exited. `pkill` and killing by name were not used.
- **Build tree removed:** `/mnt/ssd_980/herd/build/orch-308`, the
  `CARGO_TARGET_DIR` of the gate job, **13 G** at removal (`du -sh`),
  deleted before the push. The worktree.s own `target/` is kept for one
  reason only — it holds the gate **receipt** and the suite output for
  `cbf560a`, which is the evidence for this branch, and it holds no
  build output, because `CARGO_TARGET_DIR` pointed off the worktree.
