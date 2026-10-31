# orch-312-pack-by-declaration — handoff 2026-10-31

- **Item:** ORCH-312 (the packing row as it is numbered on
  `per-job-memory-ceiling`; see *Risks and findings* 1 — on `main` that
  number is a different row)
- **Agent:** herd-step
- **Branch / commit:** `orch-312-pack-by-declaration` / this commit, based
  on **`per-job-memory-ceiling` `9e1970a7cc55db910af535c20b1ee56f3103e39e`**
  and **not** on `origin/main` `f473a886e684cb9494a3c7f06a53208f0504d086`,
  as the brief directed: `job_ceiling`, `JobCeiling`, `JobNeed` and
  `CEILING_MARGIN_PERCENT` are the premise and they exist only there
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

T1 and T2 of the sizing analysis, as **one** landing, runner first.

**The one sentence.** Today's permitted total on a runner is
`seats × --job-memory-max ≤ lent − floor`; it is now
`Σ granted ≤ lent − floor`. **The same bound, finer granularity.** Every
grant is at most the cap, so the sum admits every set the count admitted
and refuses nothing the count would have taken: invariant 2's exposure
does not move. That sentence is in the code where a reader meets the
arithmetic — `herd_runner::jobs_at_once`'s doc comment,
`Session::take_job`'s own comment, `NotTaken::PastItsMemorySum`, and
`docs/runner-confinement.md` *What a runner admits against (ORCH-312)*.

**`herd-proto` — one figure, in one place (invariant 4).**

- `MINIMUM_MEMORY_MAX` **moved** from `herd-runner` to
  `herd-proto/src/declaration.rs`, beside `CEILING_MARGIN_PERCENT`, with
  `MINIMUM_CEILING_MIB = 16` as the MiB form. `herd_runner::confine`
  re-exports it, so every existing caller is unchanged. This closes the
  under-charge the brief named: `job_ceiling` floors a grant at 16 MiB, so
  a job declaring 1 MiB is *granted* 16, and a hub that charged what it
  clamped read that machine as holding one.
- `per_job_cap_mib(admissible_mib, NonZeroU32) -> u64` is
  `C_hub := (lent − floor) / at_once`. A `NonZeroU32` and no `Option`, so
  there is no default and no silence anywhere in it.
- `charged_ceiling_mib(Option<JobNeed>, Option<u64>) -> u64` is the one
  function **both sides** charge through.

**T1, the runner.**

- `JobCeiling::charged_mib()` is what a grant costs the machine;
  `JobsAtOnce::admissible_mib()` is the lend after the floor, kept from the
  same two figures the count was divided out of.
- `Session::take_job` takes a `Room { most_at_once, admissible_mib,
  grant_mib }`. Under the one lock that reads the state and writes the
  entry, it admits while `Σ granted + this grant ≤ admissible`, and
  refuses with `NotTaken::PastItsMemorySum` otherwise. `JobInHand` carries
  `granted_mib`, written when the job is taken in hand and never
  recomputed; there is nothing to release, because leaving that map is what
  gives the mebibytes back.
- **A machine too small to hold one job at its own ceiling holds one
  anyway** — `jobs_at_once`'s `.max(1)`, said over the sum: the first job
  is admitted whatever the sum says.
- The **count is kept beside it** and is the bound a host that will not say
  how much memory it lends has (`admissible_mib: None`), the figure the hub
  is still told, and the arithmetic every refusal quotes. It is **not** kept
  as a second hard bound on a host that did say its size, and the reason is
  arithmetic: `seats × cap ≤ lent − floor`, so the count would always bite
  first and the sum would be a line of code nothing can reach. That is a
  deliberate departure from the brief's "whichever bites first" on the
  runner's side, taken because the brief's own acceptance test — two
  1.7 GiB jobs where one 7 GiB seat held one — is unreachable otherwise.
- The refusal routes through `abandon_at_its_ceiling`, so it carries
  `at_its_ceiling` and **spends none of the job's goes** (D182), exactly as
  the count's refusal does.

**T2, the hub.**

- `matcher::charged_against_the_room(memory, tmpfs, room, a_reading)` is what
  one job costs a machine, and `matcher::TheRoom` is the two figures it is
  weighed against. **A costed job**: its declared ceiling — tmpfs inside it,
  margin on top, floored at 16 MiB — bounded by
  `C_hub = (lent − floor) / at_once`, this hub's only upper bound on
  `--job-memory-max`. **A job costed at nothing**: all of `C_hub`, because all
  of that cap is what the runner grants it. **A reading with no figure:
  nothing**, which is what it is charged today; a reading that *does* carry a
  figure is charged that figure's ceiling like any other job, because
  slot-free is not admission-free (D119, D159). On a runner that announced no
  count there is no `C_hub`: a costed job is bounded by the whole room and an
  uncosted one is charged nothing, which is what both were charged before.
- **The charge is the figure the job will really be held to, and that is the
  whole of why `C_hub` and not the room.** A job declaring 1 GiB of memory
  with 3 GiB of tmpfs earns a 4.4 GiB ceiling and is held to 512 MiB on a
  runner capping every job there; charging it the 1.5 GiB such a machine
  admits would be safe arithmetic and a false sentence in an operator's
  record. That was tried, between the two gate runs, and reverted
  (*Risks and findings* 3).
- `matcher::the_room(host, announced)` reads both figures off what the runner
  announced and off nothing else.
- `matcher::reserved` now takes `TheRoom` per runner, from
  `State::per_runner_rooms()` (read off the peers, because both figures are
  derived from what the *runner* announced and no job record carries them).
- `admission`'s two memory questions are now separate, which they were not:
  **can this machine honour the declaration at all** uses the *declared*
  figure, unclamped and unchanged by this work, because a machine that would
  run a 20 GiB job at a tenth of that has not run it (D180); **is there room
  now** uses the charge.

## Not done, and why

- **T3, T4 and T5 of the sizing analysis.** T3 (a reading charged nothing
  and bounded by `READINGS_AT_ONCE` alone) is not possible without one of
  the two things in *Decisions needed*, so T4's end-to-end evidence — one
  real runner holding two declared jobs where its seat count is 1 — cannot
  be produced either: the hub still offers at most `jobs_at_once`. T4's
  dependency column in the sizing analysis says T1 and T2; it is in fact
  gated on T3.
- **The throughput gain is not delivered.** `Risks and findings` 2 has the
  arithmetic. No runner holds more jobs after this landing than before it.
- `docs/status.md` is untouched: it names no ORCH-312 and no `jobs_at_once`,
  so there was no dead pointer to fix, and it is 90 lines.
- `docs/coverage.md` untouched (the brief forbade it).
- `docs/decisions.md` untouched: the entry is drafted below.

## Gates

**Two gate runs on this branch were RED and both are reported red.**

```
gate: RED sha=b87bae11bc6bf52f22755f30fa9aa37d9a41977c failed=6:test-suite
gate: RED sha=9b8ed796299b899923298f4c8920aa00a14e0809 failed=6:test-suite
gate: NOT GREEN — this sha must not be pushed to main.
```

The **first** is in *Risks and findings* 3 and is the finding of this task.
Nothing was edited to make a red assertion green: of its three failures, one
was a stale figure, one was a sentence the new charge writes differently, and
one was a wait for a refusal the new charge cannot produce.

The **second** is *Risks and findings* 10 and ORCH-324: two `busy_probe`
tests on a 20 s wall-clock deadline, in a file this branch does not touch,
green at `load1 6.53` after the first run's suite and red at `46.03` after
the second's, with the consumer's own production building on the same host.
The three tests the first run failed were green in the second.

Run on **this** tree before this file was committed, with
`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/orch-312`, each after
`./scripts/bench-preflight.sh` said `ok, safe to start`:

- `nix develop --command cargo fmt --all --check` — clean, no output.
- `nix develop --command cargo clippy --workspace --all-targets -- -D
  warnings` — clean.
- `nix develop --command cargo test -p herd-proto --lib` — `95 passed; 0
  failed`.
- `nix develop --command cargo test -p herd-runner --lib` — `316 passed; 0
  failed`.
- `nix develop --command cargo test -p herd-hub --lib` — `306 passed; 0
  failed`.
- `nix develop --command cargo test -p herd-hub --test
  packed_by_declaration` — `5 passed; 0 failed`.
- `nix develop --command cargo test -p herd-hub --test
  announced_job_ceiling` — `9 passed; 0 failed; 1 ignored`.
- `nix develop --command cargo test -p herd-hub --test fake_hub_flood` —
  `2 passed; 0 failed`.
- `nix develop --command cargo test -p herd-hub --test admission` — `5
  passed; 0 failed`.
- `nix develop --command cargo test -p herdctl --test labels_from` — `7
  passed; 0 failed`.
- `./scripts/check-docs.sh` — `docs ok`.

Four doc comments were corrected after those runs and before this commit —
`herd-hub/src/matcher.rs`'s `TheRoom`, this file's own wording and the two
test-file headers — and they change no behaviour. The run that gates the
committed tree is the one below.

**The last run, named and not quoted**: the whole suite plus the four Rust
gates, as `./scripts/gate.sh` bare and whole, submitted through Herd to
`ws://127.0.0.1:8791` with `--label build --memory-mib 4700`. Its receipt
path and its verdict are in the agent's report, not in this tree: a run made
before this file existed gated a different tree.

**Skips:** none of this branch's making. The first gate run reported
`the skip list matches the expectation exactly`, with `flood-fifty` and
`flood-two-fifty` excused by the load-conditional delta list on their own
reading of 8.85 > 8, and the `#[ignore]` list matching too. The one `ignored`
line in the runs above is
`a_link_cut_while_the_runner_is_full_loses_no_job_at_the_deployed_sixty_`
`seconds`, which carries `#[ignore]` in the tree and did before this branch.

## Risks and findings

1. **The board's numbering above ORCH-311 has collided between `main` and
   `per-job-memory-ceiling`, and this is the merger's to resolve.** On
   `origin/main` `ORCH-312` is *"a tmpfs byte a job writes IS charged to
   that job's own `memory.max`"*; on this branch's base it is *"seats are
   now pessimistic"*, the packing row, and `main` has **no** packing row at
   all. `ORCH-313`, `-314` and `-315` collide the same way. `main`'s highest
   real row is **ORCH-322** and its highest real decision is **D284**
   (re-measured; this branch's base stops at ORCH-315 and D280, because it
   was cut before batch 31 landed). The new row below is therefore numbered
   **ORCH-323** against `main`, and the done-marking of the packing row is
   applied to this branch's own board, which the merger will have to
   transplant onto `main`'s. **Nothing was renumbered inside a dated
   record**: `history/2026-10-31-per-job-memory-ceiling.md` still says
   ORCH-312 and is right about the tree it was written in.
2. **The gain this work exists for is not delivered, and the reason is one
   line of protocol.** A job costed at nothing is charged
   `C_hub = (lent − floor) / at_once`, so `n` such jobs charge at most
   `n × C_hub`, which cannot pass `lent − floor` until `n` passes `at_once`
   — the **count is what binds** for them. A costed job is charged its own
   ceiling, so the memory admission bites at that figure and not at a packed
   one. Either way **no runner holds more jobs after this landing than
   before it.** Dropping the count is what would deliver the packing, and it
   cannot be dropped while a reading is charged nothing: `reading_job` gives
   a reading no figure, and `job_ceiling` grants **every** job that arrives
   with no `JobNeed` the whole of `--job-memory-max`, because nothing on the
   frame says which jobs are readings. So the hub under-charges by up to
   `READINGS_AT_ONCE × --job-memory-max`, and the only thing covering it is
   `at_once × cap ≤ lent − floor`. An over-placement of that size is D181's
   measured refusal storm. This is ORCH-323 and *Decisions needed*.
3. **The gate was RED once, and what it found is the finding of this task.**
   The first committed tree (`b87bae11bc6bf52f22755f30fa9aa37d9a41977c`) came
   back
   `gate: RED sha=b87bae11bc6b failed=6:test-suite`
   on three tests, and two of them were the finding and not a stale string:
   `the_load_is_on_the_placement_line_and_stops_nothing` and
   `a_killed_hub_reads_the_reservations_back_off_the_records`
   (`herd-hub/tests/admission.rs`), the second of which **timed out at 30 s
   waiting for a memory refusal that could no longer happen**.

   **What it found.** Once the hub charges the ceiling the runner will grant,
   bounded by `C_hub`, `n` jobs charge at most `n × C_hub`, which cannot pass
   `lent − floor` until `n` passes `at_once` — and the announced count has
   refused by then. **So on every runner that announced a count the hub's two
   bounds coincide.** That is not a lost bound; it is the truth about a
   machine whose per-job cap is one figure: `jobs_at_once` *is* the memory
   arithmetic, and it was only ever a *separate* bound because the hub charged
   a figure (the raw declaration) that the runner would not honour. The memory
   admission is still the only bound on a runner that announced no count, and
   it still refuses a job that can never fit however empty the machine gets —
   the two things `Seat::jobs_at_once`'s own doc says it is for, both still
   asserted.

   **What I tried in between, and why it is not in the tree.** Bounding a
   costed job by the whole room instead of by `C_hub` keeps all three tests
   refusing on memory, and `cargo test -p herd-hub --lib` was green on it. It
   is wrong: that machine's 1 GiB-plus-3 GiB job is held to 512 MiB by its
   runner, so the hub would have written `1 job running there holds 1.5 GiB`
   into a record about a job holding a third of that. A safe sum is not worth
   a false figure, and the brief's own formula is the truthful one. The room
   is kept as the bound for a runner that announced no count, where there is
   no `C_hub` to use.

   **Three tests restated, each keeping the fact it was written for.**
   `the_ceiling_and_the_memory_admission_are_two_bounds_and_either_may_bite`
   now asserts the memory bound biting on a runner that announced no count,
   the two bounds coinciding on one that did — with the derivation — and the
   never-fits refusal, which is three assertions where there were three.
   `a_killed_hub_reads_the_reservations_back_off_the_records` asserted the
   inherited job's memory by submitting another gibibyte and watching it wait;
   it now asserts the figure itself, `1 job running there holds 512 MiB of
   memory and 3 GiB of tmpfs`, which is the fact under test — a reservation
   read back off a record by a hub that never saw the job start — and its
   tmpfs refusal is untouched. `the_load_is_on_the_placement_line_and_stops_
   nothing` names 512 MiB where it named 1 GiB, with the arithmetic in a
   comment.
4. **The brief's proof holds on the ordinary branch and is false on
   `.max(1)`, which the brief itself anticipated.** The first version of the
   property test asserted `charge ≥ grant` over the whole grid and went
   **red** on every triple where `lent − floor < cap`: there `at_once` is 1,
   so `C_hub = lent − floor < cap`, and the hub charges less than the runner
   grants. It is safe because the set the hub can admit has one member in
   it, which is what it admits today — so the property is now stated over
   the triples where the division did not hit its floor, and that branch has
   its own test
   (`a_host_too_small_for_one_ceiling_admits_one_job_on_both_sides`).
5. **A defect of my own, found by the suite and not by reading.** My first
   charge gave **every** reading nothing, which broke D119 and D159 —
   *slot-free, never admission-free*: a reading that carries a figure is
   charged that figure's ceiling like any other job, and
   `a_reading_is_still_admitted_against_the_room_it_runs_in` is the test that
   caught it. Only a reading **with no figure** is charged nothing, which is
   the production case and today's figure.
6. **Seven `matcher` unit tests changed their expected figures**, every one
   of them because the charge genuinely moved from the declaration to the
   granted ceiling, and each was restated with its reason rather than
   loosened. One was **renamed**, because its name had become false:
   `a_job_admitted_and_still_idle_is_costed_at_what_it_declared` →
   `..._is_costed_at_the_ceiling_it_was_granted`. One had a clause replaced
   rather than retuned (*Risks and findings* 3 has that one and the two
   `herd-hub/tests/admission.rs` ones). The third integration test restated
   is `a_reading_is_an_ordinary_job_and_the_real_job_is_placed_on_its_answer`
   (`herdctl/tests/labels_from.rs`), whose two figures are now **derived**
   from `JobNeed::ceiling_mib` in the test rather than written out, so that it
   asserts which figure won and not what the margin is.
7. **The lend is still a startup snapshot** (`herd-runner/src/room.rs`,
   D186 measured the same binaries announcing 1 seat then 2 an hour later).
   Nothing written here reads it as current, and `JobsAtOnce`'s own doc says
   so: erring low is the only direction invariant 2 allows, so a stale
   snapshot is stale downwards.
8. **`PROTOCOL_VERSION` is 3 and nothing was added to a frame.** `C_hub` is
   derived from figures already on the wire. Nothing is announced that was
   not announced before.
9. **This branch is not based on `main`**, as directed, so it carries
   `per-job-memory-ceiling`'s whole diff and a batch-30-era
   `docs/decisions.md`, `docs/orchestration/board.md` and
   `docs/orchestration/history/`. Merging it will conflict on the board. The
   full text the merger needs is below.
10. **The second gate run was RED too, and not for anything on this branch:
    the consumer's production was building on the same host.** Two
    `herd-runner/tests/busy_probe.rs` tests failed with `the probe never ran`
    — a **20 s wall-clock deadline** (`:237`, `:373`) waiting for a probe
    child to appear. Measured twice on this one branch, same code in that
    file, same day: green on both at `load1 7.88 at the start, 6.53 after the
    suite` (sha `b87bae11bc6b`, 15:14Z) and red on both at `load1 8.83 at the
    start, 46.03 after the suite` (sha `9b8ed796299b`, 15:48Z). `busy_probe.rs`
    is **untouched** by this branch (`git show --stat` names it nowhere), and
    the load was a `herd-j0000000578` job building into
    `/mnt/ssd_980/chatons/ws/target` — the consumer's own, read off its full
    `args` and never signalled (D161, invariant 9). **And a control named as
    one**: `cargo test -p herd-runner --test busy_probe` alone, on the
    committed tree, `8 passed; 0 failed` in **0.37 s** at `load1 7.04`. Two
    tests that cannot find a probe in twenty seconds inside a loaded suite
    find it in under one on a quiet host. **Recorded as ORCH-324**, with the
    gate logs as its evidence, because it is in no skip list and the next
    agent will meet it.

## Drafted for `herd-merger`

### The `docs/decisions.md` entry, in full

```
## D-NEXT — 2026-10-31 — A runner bounds itself by the sum of what it
granted, and the hub charges the same figure over the same set

ORCH-312. A job is held to its own ceiling since `per-job-memory-ceiling`
(D180), so a job granted less than `--job-memory-max` costs its machine
less than a whole seat. Two things followed and both are done here, as
one landing, because either alone is a hub and a runner counting
differently — which is the defect `herd-hub/src/matcher.rs` names in so
many words (D159, D169).

**The runner.** `Session::take_job` admits while
`Σ granted ≤ lent − floor` and refuses the job that would cross it.
Today's bound was `seats × --job-memory-max ≤ lent − floor`: the same
bound, finer granularity. Every grant is at most the cap, so the sum
admits every set the count admitted, and invariant 2's exposure does not
move. A machine too small to hold one job at its own ceiling holds one
anyway, which is `jobs_at_once`'s own `.max(1)` said over the sum. The
count is kept as the bound a host that will not say how much memory it
lends has, as the figure the hub is told, and as the arithmetic every
refusal quotes; it is not kept as a second hard bound on a host that did
say its size, because `seats × cap ≤ lent − floor` means it would always
bite first and the sum would be unreachable. The refusal spends none of
the job's goes (D182).

**The hub.** It already summed declared needs against
`admissible_memory_mib()`; it now charges `herd_proto::charged_ceiling_mib`
— the ceiling the runner will grant, tmpfs inside it and margin on top —
bounded by `C_hub := (lent − floor) / jobs_at_once`. That is this hub's
only upper bound on the runner's own `--job-memory-max`, since no frame
carries it (D180), and it is never below it, because the runner derives
the count it announced from the cap:
`at_once = max(floor((lent − floor) / cap), 1)`. So the hub's charge is
never below the runner's grant, every set the hub admits is a set the
runner accepts, and no attempt is spent on a refusal. No frame changed
and `PROTOCOL_VERSION` stays 3. A job costed at nothing is charged all of
`C_hub`, because all of that cap is what the runner grants it; on a
runner that announced no count there is no `C_hub`, and both are charged
what they were charged before.

**The charge has to be the figure the job will really be held to**, and
bounding it by the whole room instead was tried and reverted: a job
declaring 1 GiB of memory with 3 GiB of tmpfs earns a 4.4 GiB ceiling and
is held to 512 MiB on a runner capping every job there, so charging it
that machine's whole 1.5 GiB admission would be safe arithmetic and a
false sentence in an operator's record.

**One consequence, found by the gate and not by reading.** Once the
charge is the figure the runner will honour, `n` jobs charge at most
`n × C_hub`, which cannot pass `lent − floor` until `n` passes
`at_once` — so on every runner that announced a count the hub's two
bounds **coincide**. That is the truth about a machine whose per-job cap
is one figure: `jobs_at_once` *is* the memory arithmetic, and it was a
separate bound only because the hub charged a figure no runner would
honour. The memory admission is still the only bound on a runner that
announced none, and it still refuses a job that can never fit however
empty the machine gets.

`herd-hub/tests/packed_by_declaration.rs` is the proof over a grid of
triples with `herd_runner::job_ceiling` as its oracle, and it is run
twice — once over the charge the hub makes, which passes, and once over
the charge it made before, which must fail and does, on 384 triples.

**`MINIMUM_MEMORY_MAX` moved to `herd-proto`** beside
`CEILING_MARGIN_PERCENT`, as `MINIMUM_CEILING_MIB`. A grant is floored at
16 MiB, so a job declaring 1 MiB is granted 16; a hub charging what it
clamped under-charged every such job. One figure, in one place
(invariant 4).

**The upgrade order is runner first, the reverse of D181's.** D181 says
hub first, because a runner that counts fewer seats than the hub offers
refuses work. Here it is the other way round: a hub that packs beside a
runner still counting seats is over-placement, and over-placement is
D181's own measured refusal storm — eight refusals, then exit 125 with no
verdict.

**What this does not deliver, and why it is a separate decision.** No
runner holds more jobs after this than before it. A job costed at nothing
is charged `C_hub`, so `n` of them charge at most `n × C_hub`, which
cannot pass `lent − floor` until `n` passes `at_once`: the announced count
is still the bound that binds, and the hub still offers at most that many
jobs. The count cannot be dropped while a requirement reading is charged
nothing: `reading_job` gives a reading no figure (D159, D169,
deliberately), and the runner grants every job that arrives with no
`JobNeed` the whole cap, because nothing on the frame says which jobs are
readings. The hub therefore under-charges by up to
`READINGS_AT_ONCE × --job-memory-max`, and `at_once × cap ≤ lent − floor`
is the only thing covering it. ORCH-323 carries the two ways out and the
one that is Alexa's.
```

### The `board.md` rows, in full

Two changes. `main`'s board has **no** packing row, and `main` has already
used ORCH-312, -313, -314 and -315 for other subjects (*Risks and
findings* 1).

**(a)** The packing row — this branch's `ORCH-312`, which on a board based
on `main` needs a free number. Set its *State* to `done`, its *Next
action* to the text below, and append to its *Evidence*
`docs/orchestration/history/2026-10-31-orch-312-pack-by-declaration.md`
and `herd-hub/tests/packed_by_declaration.rs`:

```
**Done on `orch-312-pack-by-declaration`** (T1 and T2, one landing, runner
first): the runner admits against `Σ granted ≤ lent − floor` with the
count kept as the bound a silent host has, and the hub charges
`herd_proto::charged_ceiling_mib` — the ceiling the runner will grant —
bounded by `(lent − floor) ÷ jobs_at_once`, which is never below
`--job-memory-max`. `MINIMUM_MEMORY_MAX` moved to `herd-proto`. **What is
left is the reading charge, ORCH-323**: the hub still applies the
announced count as a hard bound, because a reading is charged nothing
while the runner grants it the whole cap, so the end-to-end gain is not
delivered yet.
```

**(b)** The new row, numbered **ORCH-323** against `main`'s highest real
row of ORCH-322. Its full text is on this branch's
`docs/orchestration/board.md`, in the *Open* section, immediately after
the packing row — copy it from there rather than from this file, so that
the pipes and the column order are the board's own.

## Decisions needed

**Question.** Dropping the hub's announced-count bound is what delivers
the packing, and it cannot be dropped while a requirement reading is
charged nothing by the hub and granted the whole per-job cap by the
runner. Two ways, and the first touches the protocol:

- **(a)** Say on the frame which jobs are readings, or carry the granted
  ceiling, so the runner charges a reading what the hub charges it.
  `PROTOCOL_VERSION` need not move — a `#[serde(default)]` field on an
  existing variant whose absence means what its default says is the first
  of the three additive shapes (D73, D124, D128) — but it **is** protocol
  surface and therefore Alexa's, asked once and not twice.
- **(b)** Charge a **figureless reading** `C_hub` as well, which every
  other job with no figure is already charged here. No frame change, and
  it is what the runner really grants one. Measured against today: on a
  memory-tight runner it is identical, because the count already binds at
  the same figure — the consumer's own 20 GiB / 6 GiB / 7 GiB gives
  `at_once` 2 and `C_hub` 7168, so two readings fill that machine either
  way; on a roomy one it is strictly better, because the count can then be
  dropped and costed jobs pack past it.

**Recommendation: (b).** It is inside the mandate, reversible, needs
nothing of Alexa and costs nothing measurable, and it makes the hub charge
what the runner grants for every job without exception — which is what
D159 and D169 ask for. Its one cost is that a reading stops being free of
the memory admission, which the brief for this task asked to preserve.

**Fallback:** leave the count as a hard bound. The landing is then a
correctness tightening and a prerequisite, and the gain waits. **On a
runner that announced a count — which every runner Herd ships does — no
machine holds fewer jobs than before**, because a charge bounded by
`C_hub` cannot refuse before that count does. A peer that announced none
is charged its declared ceiling where it was charged its declaration, so
it can hold fewer; that is the conservative direction and the only one
the hub has for a machine whose cap it cannot bound.

## Resume point

`herd-merger` takes this branch into the batch's integration together with
`per-job-memory-ceiling`, resolves the board as *Risks and findings* 1
describes, and allocates the drafted `D-NEXT`. Then ORCH-323, which is one
decision wide: answer *Decisions needed*, and if (b), change
`herd_hub::matcher::charged_against_the_room`'s `None if a_reading => 0`
arm and drop `admission`'s `if let Some(at_once) = seat.jobs_at_once`
count bound, with
`herd-hub/tests/packed_by_declaration.rs`'s
`a_reading_is_charged_nothing_and_the_announced_count_is_what_bounds_it`
as the test that has to be answered.

## Processes

**One preflight verdict was not honoured, by my own shell mistake, and it is
recorded here rather than left out.** `./scripts/bench-preflight.sh` was
chained to a control with `;` instead of `&&`, so one integration test
(`cargo test -p herd-hub --test admission`, 14.7 s) started after a `REFUSED`
verdict. Every other heavy phase in this task waited for `ok, safe to start`,
and the runs quoted under *Gates* were all made after one. The gate is a
gate; a shell operator is not an excuse for it.

Started and ended by this task: three `until ./scripts/bench-preflight.sh`
wait loops in the background, each of which exited on its own when the gate
passed, and one `Monitor` on the same condition;
`cargo` and `nix` children of the commands listed under *Gates*, each
awaited and none signalled. No process was killed, by PID or otherwise.
Nothing was written outside this worktree, this task's scratchpad
subdirectory and `/mnt/ssd_980/herd/build/orch-312`.

`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/orch-312` is off the root
filesystem and is deleted once the branch is pushed; its size is in the
agent's report.
