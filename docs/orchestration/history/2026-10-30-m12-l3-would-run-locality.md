# m12-l3-would-run-locality — handoff 2026-10-30

- **Item:** M12 task **L3** of the planner's M12 plan, §3 Track L (E-07
  must not drift from E-23). No `ORCH-` id exists for it. **ORCH-260** is
  the row about the same term being inert; this task does not close it,
  and it widens it — see *Risks and findings* 1.
- **Agent:** herd-step
- **Branch / commit:** `m12-l3-would-run-locality` / `8fc1878`, based on
  origin/main `73c2dc8`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

One commit, in `herd-hub/src/server.rs` and nowhere else. `matcher.rs`
was not opened — the brief says it is another step's — and neither was
`herdctl`, for the reason in *Risks and findings* 1.

**`would-run` on a job with `needs` names the holder and says holding is
why.** The answer now carries a line that was not there. These are the
clauses the tests assert, in the words the hub builds — a pack's id is a
sha256 and is printed whole, as every other registry sentence prints it:

```
the packs it needs, and where this hub knows them to be: kernel is the
pack <sha256>, held by tower; attic did not say what it holds, which is
not the same as holding none of this (D257)
```

and the verdict line under it, which is the matcher's own sentence
carried through and not a second copy of it:

```
it would start now, claimed by tower: … ; preference 0, the most
preferred of 2 eligible runners; it already holds the pack it needs
(kernel), so none is fetched; holding it is why it was preferred to
attic at the same preference (E-23)
```

**One read, two renderers, which is the trap this step was briefed
against.** `server.rs` gains `where_what_it_needs_is`, which asks the
registry **once** and returns three things: the `held` map the claim
step's locality term sorts on, what each `needs` key resolved to, and
which runners have said what they hold at all. `holders_of_what_it_needs`
is now one line — `where_what_it_needs_is(state, record).held` — so the
dispatch pass's input is unchanged by construction, and the dry run
renders its sentence from the same value it hands to `matcher::place`.
Nothing recomputes locality: there is one lookup and two renderings of
it.

- `held` is what the old function answered, **key for key and count for
  count**: a key that resolves to nothing, a job with no commit, a
  `(commit, key)` with two ids and a hub with no registry each still
  contribute no holding. The existing seam test,
  `what_each_runner_holds_of_a_job_s_needs_comes_off_the_registry`, is
  unchanged and still green, which is what asserts that.
- `matcher::eligible` — E-07's "would run on" set — was not touched, so
  that line is the line it was.

**And the answer that was wrong, which is the answer every job with
`needs` would get today.** A key resolves at the **dispatch**, not at
the claim: `packs_for` runs inside `dispatch`, after `matcher::place`
has claimed the job, and an unresolvable key is a wait naming the key
and the commit (ORCH-234, D13). The dry run stopped at the claim, so it
said *"it would start now"* about a job that would be claimed and then
held back — and since nothing under `herd-runner/src` constructs
`ToHub::RegisterPack` (ORCH-260), that is **every** job with `needs`.
`judge_one` now consults `packs_for` for the one job the answer is about
and says:

```
it would be claimed and then wait for its packs, <the claim sentence>.
At the dispatch, it is waiting because nothing is registered for
<commit>/kernel, which it needs: …
```

**D257, in the one place this step can print it.** A connected runner
absent from the registry's holdings map **has not said what it holds**,
which is not the same fact as holding none of this job's packs. The
placement cannot tell them apart — absent is zero for the term, and
there is nothing else it could be — so the distinction lives in the
sentence alone, as a clause of its own and never as a nought:
`cellar did not say what it holds, which is not the same as holding none
of this (D257)`. A key that resolves to no pack prints *"nothing is
registered for kernel at <commit>"* and never *"0 runners hold it"*, for
the same reason.

**A job with empty `needs` is explained byte-identically.** The locality
line is **absent** rather than empty, `packs_for` returns before it takes
a lock, and `where_what_it_needs_is` consults nothing at all — not the
fleet, not a lock, not a registry. Asserted by
`a_dry_run_of_a_job_that_needs_nothing_says_nothing_about_packs`, which
also asserts that the name still decides between two equally preferred
machines.

**Three tests, through the real `dry_run` over a real state root** — a
`JobDirStore`, its registry, a registered pack and runners registered
through `State::register_runner`, not a hand-built seat:

- `a_dry_run_says_the_holder_was_preferred_for_holding_the_key`. Two
  equally preferred runners, `attic` and `tower`. **The term has to have
  decided something or the test proves nothing**: without a holding the
  name decides and `attic` takes it, so `tower` is made the holder and
  the assertion is that `tower` is named. The pack **id** is asserted,
  not a substring of a sha256.
- `a_dry_run_says_the_dispatch_would_hold_it_back_for_a_key_nothing_registers`.
  A registry with a pack for another key, so what is missing is this
  job's key and nothing else. Asserts the key and the commit, *"it would
  be claimed and then wait for its packs"*, that *"it would start now"*
  is **absent**, and D257's clause for `cellar`.
- `a_dry_run_of_a_job_that_needs_nothing_says_nothing_about_packs`. The
  frozen half, with a pack in the registry and a holder in the fleet
  that nothing about this job may read.

Every one of them also asserts that the queue is still empty afterwards:
a dry run mints nothing (E-07).

`docs/` is untouched but for this file.

## Not done, and why

- **`herdctl` was not touched, and the brief's *Owns* names it.** It
  cannot be made to ask about a job with `needs`: `Declared::asked`
  hardcodes `needs: Vec::new()` and `leaves: BTreeMap::new()`
  (`herdctl/src/lib.rs:390`), so `Asked.needs` is empty on every
  submission and every dry run this CLI makes. Filling it changes `run`
  and `submit` and not only `would-run`, which is a change on the
  consumer's side of the fence — *Risks and findings* 1, with a board row
  drafted below and a question in *Decisions needed*.
- **No end-to-end test against the real binaries.** The plan gives
  E-23's end-to-end to **L4** and the milestone's one run to **Y1**, both
  waiting on M11's `{in}`/`{out}` layout step. And today it could not be
  written through `herdctl` at all, for the reason above: the three tests
  here drive `dry_run` over a real state root and a registered pack,
  which is the whole of the product this step changed.
- **`matcher.rs` was not opened**, by the brief. One wording defect found
  in it is reported below and left there.
- **No `docs/plan.md` or `docs/coverage.md` edit.** E-23's row belongs to
  Y1 by the plan's own *Owns*.

## Gates

**Every heavy run went through our own Herd instance**, submitted with
`/mnt/ssd_980/herd/queue/bin/herdctl`, `--label build --memory-mib 6000`,
after `./scripts/bench-preflight.sh`. **Four jobs and not one**, on the
orchestrator's instruction of this session: a single `gate.sh` job from a
cold worktree is stopped at the runner's 20 GiB disk cap. Each job
`cd`s into this worktree with
`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/m12-l3`. All four green, on the
tree of `8fc1878`, before this file existed.

| job | gate | verdict | wall | `bytes_written` |
|---|---|---|---|---|
| `j0000000042` | `cargo build --workspace --all-targets` | **exited 101** — my own compile error, see below | 73.1 s | 89,588,684 |
| `j0000000043` | `cargo build --workspace --all-targets` | exited 0 | 95.1 s | 89,867,212 |
| `j0000000045` | `cargo fmt --all --check` | exited 0 | 4.4 s | 3,489,088 |
| `j0000000046` | `cargo clippy --workspace --all-targets -- -D warnings` | exited 0 | 41.6 s | 90,178,863 |
| ``j0000000048`` | `./scripts/test.sh` | exited 0 | 341.5 s | 290,920,461 |

**`j0000000042` is reported red and it is mine, not the gate's**: one
`E0599`, `store.packs().expect(…)` on a method that answers
`&PackRegistry` and not an `Option`. Fixed, amended into the one commit,
and **resubmitted through the queue** — not re-run locally. Its figures
are kept because they are the only cold-cache reading of this branch:
`j0000000043` ran on the dep cache `j0000000042` had already written, so
**95 s is a warm build and not a cold one**.

`cargo build --workspace --all-targets` is a **superset** of the brief's
`cargo build --workspace`, named as such: it is what carries the test
targets' bytes into the first job, which is the whole point of the split.

`nix develop --command cargo build --workspace --all-targets`
(`j0000000043`) — exit 0:

```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 33s
```

`nix develop --command cargo fmt --all --check` (`j0000000045`) — exit 0,
no diff, no output.

`nix develop --command cargo clippy --workspace --all-targets --
-D warnings` (`j0000000046`) — exit 0:

```
    Checking herd-hub v0.4.1 (…/wt-m12-l3/herd-hub)
    Checking herdctl v0.4.1 (…/wt-m12-l3/herdctl)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 39.35s
```

`nix develop --command ./scripts/test.sh` (``j0000000048``) — exit 0:

```
ignored census: COMPLETE (38 ignored, 0 without a reason)
test suite: PASSED
```

**Skips — a list, never a count (D187).** **12**, and every one of them
is the standing cgroup-delegation set: this process has no delegated
cgroup v2 subtree, which is a fact about the host and not about this
branch.

```
a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device
a_declared_ceiling_changes_what_the_inert_weight_line_says
a_job_cannot_fork_past_its_own_pid_ceiling
a_job_s_cgroup_carries_the_cpu_io_and_pid_bounds
a_job_s_cgroup_carries_the_hard_io_ceiling_it_was_given
a_job_s_record_carries_the_pressure_it_ran_under
an_allocation_past_the_ceiling_kills_the_job_and_only_the_job
an_io_weight_that_cannot_bite_is_said_out_loud
a_tighter_enclosing_memory_max_lowers_the_room_that_is_announced
no_declared_ceiling_leaves_io_max_empty
no_declared_quota_leaves_cpu_max_at_the_kernel_s_max
the_runner_says_which_memory_ceiling_it_enforces
```

**The `#[ignore]` census is COMPLETE at 38**, and this branch adds none.
Measured on this tree, anchored at start-of-line, because a loose
`grep -c '#\[ignore'` answers **51** on the same tree:

```
$ grep -rn --include=*.rs '^[[:space:]]*#\[ignore' . | wc -l
38
$ grep -rn --include=*.rs '#\[ignore' . | wc -l
51
```

`./scripts/check-docs.sh` — exit 0, `docs ok`, on the tree of `8fc1878`
before this file existed.

**The last run, named and not quoted:** `./scripts/check-docs.sh` on the
tree of the commit that carries this file, its output in this agent's
report to the orchestrator. Nothing is added to the tree afterwards.

## Risks and findings

1. **`herdctl` parses `needs` and `leaves` and then throws them away, so
   ORCH-260's diagnosis is one of two breaks and not the only one.** That
   row says the term is inert because nothing constructs
   `ToHub::RegisterPack`; measured on this branch's base `73c2dc8`, there
   is a second, **earlier** break in the same chain.
   `herdctl/src/declaration.rs` reads both fields off `commands.toml`
   (`:589` the file's own field, `:332` the checked `Command`, `:259` the
   `Resolved`, filled at `:490`) — and `Resolved::declared()` at `:291`
   carries neither onto `crate::Declared`, which has no field for either,
   and `Declared::asked` at `herdctl/src/lib.rs:390` writes
   `needs: Vec::new()` and `leaves: BTreeMap::new()`. So
   `Asked.needs` is empty on **every** submission and **every** dry run
   this CLI makes, `JobRecord::needs` is empty on every record, and the
   hub's whole `needs` path — `packs_for`, `holders_of_what_it_needs`,
   the locality term, and this step's own sentence — is reachable only
   from a client that is not `herdctl`. **The zero is measured, not
   assumed:** the same grep finds `needs:` filled at
   `declaration.rs:490` and `:643`, so the detector fires where the
   field *is* carried.
2. **The claim sentence says *"and fetches it"* of a pack that may not
   exist**, and `matcher.rs` is not mine to edit.
   `matcher.rs:1458`'s `(0, true)` arm prints *"it does not hold the pack
   it needs (kernel) and fetches it"* whenever the count is zero —
   including when the key resolves to nothing at all, which is every job
   today. In a dry run the clause this step adds now corrects it two
   lines later (*"At the dispatch, it is waiting because nothing is
   registered for …"*), but on a **real** placement the `ran on:` line
   says it unqualified. The line is pinned by
   `matcher.rs:4646`'s assertion, so moving it is a matcher task with its
   own frozen-text decision, not a wording fix.
3. **One extra registry read per queued job *with* `needs`, per pass.**
   `where_what_it_needs_is` now also calls `PackRegistry::holdings()`,
   which takes the registry's mutex once and clones one `(name, count)`
   pair per announcing runner, to answer D257's question. A job with no
   `needs` returns before any of it, so **today's queues pay nothing**:
   no job Herd runs has `needs` (finding 1). Nothing here was timed; it
   is derived from the code path and said as such.
4. **`bytes_written` is not a measure of what a job writes, and this
   branch's figures say so again (ORCH-268).** `j0000000043` recorded
   **89,867,212** bytes — 85.7 MiB — while the tree it wrote into
   measured **14 GB** (`du -sh` on `/mnt/ssd_980/herd/build/m12-l3`,
   `debug` profile with test targets, after the build job). The figures
   in the table are reported because the orchestrator asked for them as
   sizing evidence; they are **two orders of magnitude below** the bytes
   really written, so a disk cap read against them would never fire.
5. **A runner mid-announcement counts as having said.**
   `PackRegistry::announced(…, more: true)` creates the runner's holdings
   entry and extends `arriving`, leaving `held` at the previous complete
   set, so `holdings()` lists it and D257's clause does not name it. That
   is the registry's own design — a chunked announcement is applied
   whole or not at all — and the sentence inherits it. A runner whose
   **first** announcement is still arriving therefore reads as holding
   none rather than as silent, for the length of that announcement.
6. **The dry run now reads manifests, through `packs_for`.** Reads only,
   and only for the one job the answer is about; E-07 forbids writes and
   nothing here writes. `owner_control.rs`'s byte-for-byte state-root
   assertion is unaffected: it dry-runs a job with no `needs`, for which
   `packs_for` returns before it touches the disk.

## Drafted for `herd-merger`

**For `docs/decisions.md`** — append verbatim, allocating the next free
number in place of `D-NEXT`:

```markdown
## 2026-10-30 — D-NEXT A dry run answers for the dispatch too

E-07 promises where a job would run. Until M12's L3 that answer stopped
at the claim step, and a job with `needs` resolves its keys **after**
the claim: `packs_for` runs inside `dispatch`, not inside the claim, so
a job the fleet fits and the registry cannot serve is claimed, held
back by name, and left queued — while `would-run` said *"it would
start now"* about it.

That is not a corner. Nothing under `herd-runner/src` constructs
`ToHub::RegisterPack` (ORCH-260), so **every** job with `needs` is in
exactly that state today, and the dry run was wrong about all of them.

**The rule: a dry run consults every step a submission would pass
through, including the ones after the claim, and consults them
read-only.** `would-run` now asks `packs_for` for the one job it is
about and answers *"it would be claimed and then wait for its packs"*,
naming the key and the commit. A job that needs nothing reaches none of
it — `packs_for` returns before it takes a lock — so that answer is the
bytes it was.

**And how two renderers of one decision are kept from drifting.** The
matcher's placement line and the dry run's explanation read one value:
`where_what_it_needs_is` asks the registry once and returns the `held`
map the locality term sorts on, what each key resolved to, and which
runners have said what they hold at all (D257's distinction, which the
placement cannot make and the sentence must). A dry run that computed
its own locality would agree today and drift at the next change to
either side.

Reversible: both halves are reads and neither writes. The cost is one
registry lock per queued job **with `needs`** per pass, which is no job
Herd runs today.
```

**For `docs/orchestration/board.md`** — one new row, in *Open*, with the
next free `ORCH-nnn` in place of `ORCH-NEXT`:

```markdown
| ORCH-NEXT | **`herdctl` parses `needs` and `leaves` and then throws them away, so E-23's locality term cannot be reached from the CLI at all.** ORCH-260 names one break in the chain — nothing constructs `ToHub::RegisterPack` — and there are **two**. Measured at `73c2dc8`: `herdctl/src/declaration.rs` reads both fields off `commands.toml` (the file's own field at `:589`, `Command` at `:332`, `Resolved` at `:259`, filled at `:490`), and then `Resolved::declared()` at `:291` does not carry either onto `crate::Declared`, which has no field for them; `Declared::asked` at `herdctl/src/lib.rs:390` hardcodes `needs: Vec::new()` and `leaves: BTreeMap::new()`. So `Asked.needs` is empty on every submission **and** every dry run this CLI makes, `JobRecord::needs` is empty on every record, and the hub's whole `needs` path — `packs_for`, `holders_of_what_it_needs`, the locality term, M12 L3's explanation — is reachable only from a client that is not `herdctl`. The hub side is complete and tested; the wire carries the fields at `PROTOCOL_VERSION` 3 and the record at `RECORD_VERSION` 2 | `m12-l3-would-run-locality`, 2026-10-30, finding 1; measured at `73c2dc8` | herd-planner | open | **Not a one-line fix and not L3's to take, because filling it changes `run` and `submit` and not only `would-run`.** `Declared::asked` is one conversion on purpose — it is what makes a dry run answer about the job a submission would really make (E-07, and `server.rs`'s *"rather than answering about a job nobody can submit"*) — so a `--needs` flag on `would-run` alone is refused by that design, and filling the two fields makes every declared command that already names `needs` start asking for packs. On a hub where nothing registers one, those jobs would go from running to waiting at the dispatch, which is a change on the consumer's side of the fence (D161). Fold it into M11's `{in}`/`{out}` layout step beside ORCH-260, which is where `RegisterPack` is constructed for the first time, and land the two halves together or neither | M11's `{in}`/`{out}` layout step; M12 L4, which needs `leaves` to travel for its fixture declaration | `herdctl/src/declaration.rs:259`, `:291`, `:332`, `:490`, `:589`; `herdctl/src/lib.rs:340` `Declared`, `:376` `asked`, `:390`; `herd-proto/src/lib.rs:2229` `Asked::needs`; ORCH-260; **D255** |
```

**And one row to amend, not to add.** **ORCH-260**'s cell says the term
is inert because nothing constructs `ToHub::RegisterPack`. That is true
and it is half: with the row above landed beside it, ORCH-260's *Next
action* — *"fold into M11's `{in}`/`{out}` layout step"* — should name
both halves, because registering a pack does not make the term
observable while `herdctl` sends no `needs`. No text for ORCH-260 is
drafted here: it is an append-only cell of somebody else's row, and the
new row cites it.

## Decisions needed

**Question.** Does `herdctl` start carrying a declaration's `needs` and
`leaves` onto `Asked` now, or with M11's `{in}`/`{out}` layout step?
**Recommendation: with the layout step, both halves in one landing.**
Filling it alone turns every declared command that already names `needs`
from a job that runs into a job that waits at the dispatch, on a hub
where nothing registers a pack — a change on the consumer's side (D161),
and one nobody would see coming from a CLI commit. **Fallback**, if L4
needs it sooner: land it behind the registration path in the same batch,
never in front of it. Not Alexa's: it is *how*, not *what*.

## Resume point

M12 **L4** — the `produce` job end to end — which the plan already parks
behind M11's `{in}`/`{out}` layout step, and which **also** needs the
`herdctl` half of finding 1 before its fixture declaration's `leaves` can
travel. Whoever takes the layout step should take both halves of the
chain in one go: `ToHub::RegisterPack` constructed on the runner, and
`Resolved::declared()`/`Declared::asked` carrying the two fields.

## Processes

- No PID was signalled. No `pkill`, no `killall`, nothing by name.
  Nothing of the consumer's was touched: every compile and the suite ran
  **inside a Herd job**, under the runner, not under this agent's shell.
- Started and ended by this agent: five `herdctl run` clients, one per
  gate job, each exiting with its job's verdict; one `until` loop on
  `/proc/loadavg`, waiting for the preflight to clear, which exited on
  its own condition.
- **`bench-preflight.sh` refused once and was obeyed**: load 13.24
  against a ceiling of 9.00 on 12 cores, with another agent holding the
  queue's second build slot. The suite was not submitted until a second
  reading cleared at load 6.54.
- Build tree: `/mnt/ssd_980/herd/build/m12-l3`, **16 GB**, `debug`
  profile with test targets — the figure named with its profile, because
  the other profile is forty times smaller. Deleted once the branch was
  pushed; the deletion is in this agent's report.
