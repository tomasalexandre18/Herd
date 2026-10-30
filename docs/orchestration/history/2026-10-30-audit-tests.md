# audit-tests — handoff 2026-10-30

- **Item:** ORCH-197 (test one), ORCH-NEXT board rows below (tests two and
  three); the two audits of 2026-10-30, findings F1, F2 and B2
- **Agent:** herd-step
- **Branch / commit:** `audit-tests` / `a6855c6` for the three tests, plus
  this handoff as a fourth commit, based on
  `origin/store-guards` `63a9aff`, itself a descendant of `pack-registry`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

Three tests, three commits, **no production line changed** in any of them.
Verified mechanically rather than claimed: the only hunk in
`herd-hub/src/store.rs` is `@@ -6760,0 +6761,137 @@ mod tests`, a pure
insertion, and `head -5366 herd-hub/src/store.rs | sha256sum` is
`af18b10450bcae53f874dff9ff04f3b50e7a940913974772802e9c6c9759b240` on both
the pristine file and the committed one.

### Commit one — `herd-hub/tests/owed_scan_fails_against_the_real_runner.rs`

The real runner's barrier against a real hub whose `readdir` fails
(audit one's F1, its highest-ranked finding).

- `Herd::new()`, the real `herd-hub` and the real `herd-runner` as
  processes, from `real_herd/mod.rs`.
- `SWEPT = herd_runner::FILES_IN_FLIGHT + 4` = **12** collections, each two
  files of `(1 << 20) + 7` bytes, the hub **owed** every byte of all of
  them: a first hand-driven life runs twelve jobs to a verdict announcing a
  streamed collection and sends no chunk.
- The spools are planted on the runner's data root, the root is made to
  believe it belongs to this hub's state, and only then is the hub's `jobs/`
  put at `0o111` — submitting needs a `mkdir` there, everything after it
  needs only to traverse.
- The real binary then goes through its own start-up: `sweep::pick_up`,
  `settle_against_state_root`, `offer_swept_collections`, its own health
  interval, its own `the_hub_has_said_what_it_is_owed`.

Four things asserted, in this order, because the last one only means
something if the first three hold:

1. the hub said in its own words that it **cannot tell which collections
   are outstanding** — without this line the listing quietly worked and a
   green reading is about nothing;
2. the unprompted *"still owes this hub files of …"* line appears for
   **none** of the twelve, so the silence at join is real;
3. `that job's own record answered what the scan of all of them could not`
   appears **exactly twelve times** — D225's per-offer answer, once per
   collection;
4. **nothing is condemned**: no `.unclaimed`, no spool left behind, zero
   barrier lines, and every byte of every collection whole in the hub's
   `files/` afterwards.

The module header states, at length, **why a hand-driven peer could not
have caught this and must not be "simplified" back to one**: a hand-written
peer has no pump, no window of eight permits, no health interval and no
barrier, so the sequence it asserts is the sequence its author typed. The
same paragraph was added to `owed_scan_fails.rs`'s own header, pointing at
this file.

The `0o111` fixture moved into `herd-hub/tests/a_failed_listing/mod.rs` and
`owed_scan_fails.rs` now uses it, so the permission, the `Drop` guard that
gives it back, the *"but a record still opens"* half and the D27 skip
sentence are **one definition for both files** rather than two copies. Net
effect on `owed_scan_fails.rs`: 45 lines of duplicated fixture removed, its
three skip call sites now naming
`AListingThatFails::WHY_IT_WOULD_BE_SKIPPED`.

### Commit two — the hub's startup sweep, asserted at its entry point

In-crate, as the brief judged: the pair is hub-internal and needs no
process. A job with a part-written collection is left on a root by a store
that is then **dropped** (D21's one lock), and the root is reopened with
`JobDirStore::open_with` on a budget of 4 bytes against the 8 it holds —
so the `recover` / `sweep_outputs` pair three lines apart in `open_with`
has already run by the time the next line executes. It asserts the arrived
bytes, `record.outputs.removed.is_none()`, that `collections_owed()` still
names exactly that job, and `could_not_free == 8`.

The clause has moved since the audit read it: `store-guards` added the
`check_relative` call inside `collection_is_whole` an hour earlier, and the
line is now **`store.rs:973`**, not `:883`.

### Commit three — one `--help` test per binary

`the_long_help_states_every_figure_its_constants_decide`, in
`herd-hub/src/main.rs`, `herd-runner/src/main.rs` and
`herdctl/src/main.rs`.

**Why in `main.rs` and not in `tests/`:** the `Args` struct and
`herdctl`'s `EXIT_CODES` are private to each `main.rs`, so
`Args::command().render_long_help()` is reachable only from an in-crate
`#[cfg(test)]` module. All three files already had one, and two already
used `clap::CommandFactory`. An integration test would have had to spawn
the binary and parse its stdout, which is a second thing to get wrong for
no gain.

Every figure arrives through the constant and nothing is retyped:

| binary | phrase checked | read from |
|---|---|---|
| runner | `… by default, and the flag` | `herd_proto::show_bytes(DEFAULT_OUTPUT_CAP)` |
| runner | `INLINE_OUTPUT_MAX`, …)` | `show_bytes(herd_proto::INLINE_OUTPUT_MAX)` |
| runner | `… to … against the 100 an ordinary process has. … by default` | `cgroup::WEIGHT_RANGE.start()`/`.end()`, `DEFAULT_CPU_WEIGHT` |
| runner | `… to … against 100. … by default` | the same range, `DEFAULT_IO_WEIGHT` |
| runner | `… by default` (pids) | `DEFAULT_PIDS_MAX` |
| runner | `a quarter of whatever it is is kept back` | `BUILD_SHARE_DENOMINATOR - BUILD_SHARE_NUMERATOR` over `BUILD_SHARE_DENOMINATOR`, spelled by a table whose fallback arm panics |
| runner | `a fifth of the memory, between 1 GiB and 8 GiB, when not given` | `default_memory_floor_mib` **probed**: the clamp's two ends at the extremes, and the denominator found by searching for the smallest host size whose floor falls strictly inside the clamp and divides it exactly |
| hub | `at least every 60 s` | `WAIT_RESTATEMENT.as_secs()` |
| hub | `fifteen seconds, the same number as` | `STILL_THERE_EVERY`, plus `assert_eq!(STILL_THERE_EVERY, HEALTH_EVERY)` — the sentence's own claim of equality, asserted against both constants |
| hub | `fifteen minutes, the figure E-27 names` | `DRAIN_FIRST_LIMIT.as_secs() / 60` |
| hub | `five minutes by default` | `READING_CAP.as_secs() / 60` |
| hub | `two by default` | `READINGS_AT_ONCE` |
| hub | `Nothing computed 200 GB` | `show_bytes(OUTPUT_BUDGET)` |
| hub | `seven days` | `OUTPUT_FLOOR.as_secs() / 86400` |
| herdctl | `124 a runner stopped the job at a cap` | `herdctl::STOPPED_BY_CAP_CODE` |
| herdctl | `125 Herd has no verdict` | `herdctl::NO_VERDICT_CODE` |
| herdctl | `128 + n the job was killed by signal n` | `exit_code_for(&Signalled { signal: 9 }) - 9` — the base **asked of the function**, never retyped |
| herdctl | `2 a command did part of what it says` | `NOT_SETTLED_CODE` |
| herdctl | `exit of 124 or 125 from Herd's` | both codes |
| herdctl | the two `{"exit_code": 124, "exit_code_from": …}` worked examples | `STOPPED_BY_CAP_CODE` |
| herdctl | `Falling back after 125:` and `125 is one code for two opposite situations` | `NO_VERDICT_CODE` |

Two devices make this hold rather than look like it holds. Whitespace is
**flattened to one line** before the comparison, because clap re-wraps help
text to a width a test has no business knowing and a sentence that straddles
a line break is still that sentence. And the two word tables —
`a_number_in_words`, `a_fraction_in_words` — **panic** on a figure they have
no word for, with a sentence saying to read the help rather than to edit the
test: a constant moved to a value nobody spelled out is a loud failure, not
a silent pass.

## Not done, and why

- **Nothing of the three was dropped.** All three tests are written, green,
  and each was demonstrated red by breaking exactly the thing it guards and
  then restored to a byte-identical sha256.
- **`DEFAULT_LOG_CAP` and `--health-every` are not asserted**, because
  neither states a figure in prose for a test to check. Finding F-b has the
  text of both blocks. Not a gap; a mis-enumeration in the brief.
- **One figure in `herd-hub --help` is not asserted**: the exit code 125 in
  `--still-there-every`, which belongs to `herdctl` and is out of reach
  from a crate that does not depend on it. Finding F-d, with a board row
  proposing the one-line fix.
- **Audit one's F3 to F6 are untouched** — they were not in this brief.
  Board row three ranks them, with F3 first because it is the only
  wrong-bytes failure mode on the audit's list.
- **No production line changed, anywhere**, which was the brief's hard
  constraint. `herd-hub/src/store.rs`'s only diff hunk is an insertion
  inside `mod tests`; the three `main.rs` additions are inside their
  existing `#[cfg(test)] mod tests`. The two temporary clause deletions and
  the three temporary help-string edits made for the red demonstrations are
  all restored, each verified by `sha256sum -c` rather than by inspection.
- **`scripts/`, `docs/*`, `.github/`, `docs/decisions.md` and
  `docs/orchestration/board.md` untouched**, as the brief requires; the
  decision entry and the three board rows are drafted in full above (D226).

## Gates

All four Rust gates plus `check-docs.sh`, on the tree of `a6855c6` (the
three commits) — run **before** this file was written, which is why their
output can be quoted here.

`nix develop --command cargo build --workspace --all-targets`:

```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.77s
```

`nix develop --command cargo clippy --workspace --all-targets -- -D warnings`:

```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 18.07s
```

`nix develop --command cargo fmt --all --check`: clean. It was **not** clean
on the first attempt — rustfmt wanted two `format!` calls in `herdctl`'s
test reflowed — so `cargo fmt --all` was run, the result rechecked clean,
and the change amended into the third commit rather than left as a fourth.

`nix develop --command ./scripts/test.sh`
(`…/scratchpad/at-suite.log`):

```
test suite: PASSED
```

**Skips: 12, and the same 12 names `scripts/expected-skips/local.txt`
carries** — every one a cgroup-v2 delegation or hard-IO-ceiling assertion
this host cannot make. **None comes from the three new tests.** Printed as
skipped and not counted as passed (D27):

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

The new test's own D27 skip
(`a_real_runner_condemns_nothing_when_the_hub_cannot_list_its_jobs`, for a
process file permissions do not apply to) **did not fire** and cannot on
this host: uid 1002, and the three existing `owed_scan_fails` tests build
the same fixture and pass. So `scripts/expected-skips/` needs no new name
and `scripts/` is untouched.

`./scripts/check-docs.sh`:

```
docs ok
```

### The figure asked for before anything was written

`nix develop --command cargo test -p herd-hub --lib`, on the unmodified
base `63a9aff`, before a line of this branch existed:

```
test result: ok. 257 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 9.00s
```

After test two: **258 passed; 0 failed; 0 ignored**, 9.05 s. A real test
target that compiled and ran, not `cargo check --lib`.

### Test one, five runs, verbatim

`cargo test -p herd-hub --test
owed_scan_fails_against_the_real_runner -- --nocapture`, under
`nix develop --command`, five separate invocations:

```
owed-scan-fails-real-runner: 12 collection(s) owed and swept, 0 set aside, 0 still spooled, 12 answered from their own records, 0 condemned by the barrier
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 22.49s

owed-scan-fails-real-runner: 12 collection(s) owed and swept, 0 set aside, 0 still spooled, 12 answered from their own records, 0 condemned by the barrier
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 17.89s

owed-scan-fails-real-runner: 12 collection(s) owed and swept, 0 set aside, 0 still spooled, 12 answered from their own records, 0 condemned by the barrier
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 18.08s

owed-scan-fails-real-runner: 12 collection(s) owed and swept, 0 set aside, 0 still spooled, 12 answered from their own records, 0 condemned by the barrier
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 17.74s

owed-scan-fails-real-runner: 12 collection(s) owed and swept, 0 set aside, 0 still spooled, 12 answered from their own records, 0 condemned by the barrier
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 17.89s
```

**Swept count: 12 every run** (`herd_runner::FILES_IN_FLIGHT + 4`), 0 set
aside, 0 still spooled, **12 answered from their own records** every run, 0
barrier lines. A sixth run inside `./scripts/test.sh` makes six.

### Test two, the delete-the-clause demonstration

Two deletions, because one is not enough to lose a byte. Both in finding
F-a with their output. `store.rs` restored and verified:

```
$ sha256sum herd-hub/src/store.rs
2523ec95f161dae5991e71a6608012a941400d18924553fc71230dd526b20c12  herd-hub/src/store.rs
$ head -5366 herd-hub/src/store.rs | sha256sum
af18b10450bcae53f874dff9ff04f3b50e7a940913974772802e9c6c9759b240  -
$ git diff --unified=0 herd-hub/src/store.rs | grep '^@@'
@@ -6760,0 +6761,137 @@ mod tests {
```

The second sha256 is the production half of the file — every line above
`mod tests` — and it is **byte-identical to the pristine `63a9aff`
file**. The only hunk is a pure insertion of 137 lines inside the test
module.

### Test three, the change-a-figure demonstration

One figure broken per binary, each run on its own, each restored:

```
`herd-runner --help` must state --job-pids-max's default as its constant decides it, and the rendered help does not contain "1024 by default".
`herd-hub --help` must state --still-there-every's cadence as its constant decides it, and the rendered help does not contain "fifteen seconds, the same number as".
`herdctl --help` must state the cap row as the code decides it, and the rendered help does not contain "124 a runner stopped the job at a cap".
```

(`/// 1024 by default.` → `2048`; `acts on the figure. Fifteen` →
`Thirty`; the exit table's `124` row → `123`.) Restored and verified:

```
$ sha256sum -c at-mains-sha.txt
herd-runner/src/main.rs: OK
herd-hub/src/main.rs: OK
herdctl/src/main.rs: OK
```

### The last run, named and not quoted

`./scripts/gate.sh`, bare — **never inside `nix develop`**, which nests
`TMPDIR` and panics `confinement.rs` on the 108-byte `SUN_LEN`. Run on the
committed tree including this file, with its output at
`…/scratchpad/at-gate.log`. The verdict is in my report to the
orchestrator, not in the tree it gated. The branch is pushed only on green.

### The host, and the margin I started on

`./scripts/bench-preflight.sh` before every heavy phase, as its own
command. The margin I waited for, taken from another agent's measurement
tonight rather than from a threshold: **≥ 4608 MiB above the script's own
8192 MiB floor on two readings 20 s apart.** What I started on:

```
03:11:00 load=4.27 room=14794 verdict=ok
03:11:20 load=3.54 room=14744 verdict=ok
```

**A plateau, not a peak**, and the earlier four readings say so: 14700,
14726, 14718, 14717 MiB across 02:54:15–02:55:15 on the first attempt, and
12487–12928 MiB through every phase after — never below ~4.2 GiB of
headroom. **This figure is not a threshold and must not be read as one**:
at 03:10:20 the same script said *"ok, safe to start"* on room=10071, which
is 1879 MiB above its floor, and starting there would have been wrong. The
margin is the rule; 14744 is only what this night happened to offer.

## Risks and findings

### F-a. **Audit one's F2 is refuted twice over, and the second
### refutation is the interesting one**

The brief, quoting audit one's F2: *"delete
`|| !self.collection_is_whole(&id, &record)` and nothing anywhere goes
red."* Measured, on `origin/store-guards` with that clause deleted and
nothing else changed:

```
---- store::tests::a_collection_still_arriving_is_never_swept stdout ----
thread 'store::tests::a_collection_still_arriving_is_never_swept' panicked at herd-hub/src/store.rs:6757:9:
assertion `left == right` failed: the figure is reported rather than hidden, because an operator looking at a volume that will not come down has to be able to see why: Sweep { removed: [], freed: 0, held: 8, inside_the_floor: 0, could_not_free: 0 }
  left: 0
 right: 8
test result: FAILED. 256 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 8.89s
```

So one test does go red. **And it is the test the brief said does not
exist.** The brief says the two nearest tests are "that a collection still
owed is derived from the records" and "that an evicted one is owed by
nobody", and that *"neither asserts that an incomplete collection is not a
sweep candidate"*. `store::tests::a_collection_still_arriving_is_never_swept`
(`store.rs:6718`) asserts exactly that, has done since D152, and the audit's
own table does not mention it. The brief looked at the wrong two tests.

**The second half matters more.** With the clause deleted the **bytes are
not lost**: `JobDirStore::evict` carries its own, independent
`|| !self.collection_is_whole(id, &record)` (`store.rs:790`), so the
candidate is pushed, `evict` answers `Ok(None)`, the loop continues, and
nothing is evicted and nothing marked `removed`. The only observable change
is the accounting figure. Measured by deleting **both** clauses, which is
what it takes to lose a byte:

```
---- store::tests::a_collection_still_arriving_is_never_swept stdout ----
herd-hub: the output sweeper removed 1 collection(s), 8 bytes, to stay inside the 4 bytes budget; 0 bytes held now.
thread 'store::tests::a_collection_still_arriving_is_never_swept' panicked at herd-hub/src/store.rs:6747:9:
and the half that did arrive is still here
test result: FAILED. 256 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 8.89s
```

**What this changes about F2's severity.** The audit rated it *"highest if
it ever breaks, and it is one `||` wide"*. It is two `||`s wide, in two
functions, and the one the audit named is the **accounting** clause rather
than the byte-safety one. D211-on-the-hub needs both to go. That is a
narrower risk than the audit states, and the right correction is to the
*reason* rather than to the work: the gap was never the clause, it was that
**every** test of the sweeper — all eight of them — calls `sweep_outputs()`
on a store that is already open, and nothing exercised the `recover` /
`sweep_outputs` pair through `open_with`. That is what the new test does,
and it is why it asserts both halves: `could_not_free` bites on the named
clause, and the bytes, `removed` and `collections_owed()` bite on `evict`'s.

### F-b. **The brief over-enumerates test three's constants,
### against its own source**

Two of the constants the brief names cannot be asserted against any help
text, and in both cases audit two's own table agrees with the code rather
than with the brief:

- **`DEFAULT_LOG_CAP`.** `--job-log-cap`'s whole doc comment is *"How much
  log one job may produce before the runner stops it."* No figure. Its
  default does not even reach the rendered help: the flag is
  `Option<String>` with no `default_value_t`, so clap prints no
  `[default: …]` either. The audit's 11-row table does not list it; the
  brief added it.
- **`--health-every`.** *"Seconds between health reports (E-40)."* No prose
  figure. The figure reaches the help only as clap's `[default: 15]`,
  rendered from `HEALTH_EVERY.as_secs_f64()` — already derived from the
  constant, which is the thing this test exists to enforce. The *spelled*
  fifteen lives in **the hub's** `--still-there-every`, which is where the
  new test checks it, together with the claim that the two numbers are the
  same.

Neither is a defect. Both are blocks with nothing to disagree with.

### F-c. **Four figure-bearing clap blocks the audit's table
### missed, all correct, two of them now gated**

Audit two says *"23 figure-bearing clap doc blocks and all 23 are correct"*
but tabulates only 11. Re-scanning the three `main.rs` files for a prose
figure next to a unit, a range or *"by default"* (detector:
`…/scratchpad/at-help2.pl`) finds four more that state a figure a constant
decides:

| block | prose | constant | verdict | gated now |
|---|---|---|---|---|
| hub `--drain-first-limit` | *"Fifteen minutes, the figure E-27 names"* | `matcher.rs:275` `15 * 60` | correct | **yes** |
| hub `--reading-cap` | *"Five minutes by default"* | `server.rs:310` `5 * 60` | correct | **yes** |
| runner `--drain-grace` | *"Must stay under the five seconds invariant 2 promises the owner"* | invariant 2, **not** `DRAIN_GRACE`, which is `lib.rs:116` **3 s** | correct as written | no — see below |
| runner `--switch-poll` | *"A second by default, against E-33's ten seconds"* | `switch::Poll::default()`; E-33's figure is the consumer's requirement | correct | no |

**No figure in any of the three binaries' help is wrong.** I checked every
pairing before asserting it, and the first full run of all three tests
passed, which is the same statement made by a machine.

The `--drain-grace` row is the one worth a sentence: *"five seconds"* there
is invariant 2's promise and `DRAIN_GRACE` is three. A test that paired them
— the obvious thing to write — would have asserted a falsehood and then been
"fixed" by editing the help. It is named in the test's doc comment as
deliberately not asserted, with that reason.

### F-d. **One figure in `herd-hub --help` has no gate reachable
### from where it is written**

`--still-there-every` explains itself with *"which `herdctl` reads as the
stream ending with no verdict and reports as exit 125 on a job that
succeeded"*. **125 is `herdctl::NO_VERDICT_CODE`**, and `herd-hub` does not
depend on `herdctl` — so the hub's own test cannot read that constant. The
figure is asserted in `herdctl`'s test against `herdctl`'s constant, and the
copy of it in the hub's help is still correct by attention. It is one
cross-crate figure, in a sentence nobody acts on, so this is a finding and
not a defect; a board row below proposes where it could go.

### F-e. `scripts/` needs nothing, verified rather than assumed

`gate.sh` is red on any skip not in `scripts/expected-skips/local.txt`, so a
new `herd_runner::testing::skipped` call site is a gate risk. The new test's
skip fires only for a process file permissions do not apply to; this host is
uid 1002, the three existing `owed_scan_fails` tests pass here, and
`local.txt`'s 12 names contain nothing about a listing. No new skip appears
and `scripts/` — off limits to me anyway — needs no edit.

### Risks

- **Test one is a race, and five green runs on one host are five
  measurements of that host.** They are reported individually and the test
  prints its figures whichever way the assertion goes, so a future red run
  carries its own diagnosis. The scale is the thing that keeps it honest:
  12 collections against a window of 8, with the reason written into the
  constant's doc comment so nobody lowers it.
- **Test one costs about 18 s** (five runs: 22.49, 17.89, 18.08, 17.74,
  17.89 s; the first includes a build). It is one more real-binary test in
  a suite that already has several, and it is not `#[ignore]`d — a gate the
  batch runs anyway.
- **The shared fixture module touches a green test.** `owed_scan_fails.rs`
  was edited to use `a_failed_listing/mod.rs` instead of its own copy. The
  change is mechanical, the suite covers it, and the alternative was a
  second copy of a fixture in a batch whose other audit is about exactly
  that.

## Drafted for `herd-merger`

One `docs/decisions.md` entry and three `board.md` rows, in full.

### Decision entry

```markdown
## 2026-10-30 — D-NEXT A figure in `--help` is read back through its own constant

**Context.** `--help` is the first operator surface anybody reads and it
was the only one in the repository with no gate of any kind. `herd-proto`
has `frozen_formats.rs` and `frozen_verdict.rs`, `size.rs` has golden
strings, `confine.rs` has `const _: () = assert!(…)` across crates,
`release.yml` reads the version constants out of the source — and the
figures in the three binaries' clap doc comments were correct by
somebody's attention. Measured over `git ls-files '*.rs'`, 105 files:
**zero** occurrences of `--help` as an argv string anywhere.

Attention is what failed. `herd-runner --help` described
`--job-output-cap` as *"32 MiB by default, which is also the most this can
be"* against a `DEFAULT_OUTPUT_CAP` of `1 << 30` — wrong by 32×, inverted
about what the flag does, wrong about the consequence — and it shipped in
**v0.3.7, v0.4.0 and v0.4.1**. It was closed by a human reading
(`7c75525`, 2026-10-29), and the document that reported it was not
cleared. An operator who sets `--job-output-cap 32MiB` because the help
said that was the ceiling gets a cap a thirty-second of what they meant,
on a machine nobody in this project may log into (D161).

**Choice.** One test per shipped binary,
`the_long_help_states_every_figure_its_constants_decide`, rendering
`Args::command().render_long_help()` and asserting that each figure the
prose states is present **as the constant's own formatter prints it** —
`herd_proto::show_bytes`, a `RangeInclusive`'s own ends, a `Duration`'s
own `as_secs`, a fraction obtained by dividing, an exit code asked of
`herdctl::exit_code_for`. No figure is written out a second time: a test
carrying its own copy of the number is a fourth place for the number to
be wrong, and would go green on the day the constant moved.

It lives in each `main.rs` because `Args` and `herdctl`'s `EXIT_CODES` are
private to it; all three files already had a `#[cfg(test)]` module and two
already used `clap::CommandFactory`. Whitespace is flattened before
comparison, because clap re-wraps to a width a test has no business
knowing. Where the help spells a figure in words, the word is produced
from the constant by a small table whose fallback arm **panics** with a
sentence saying to read the help rather than to edit the test.

**What it does not cover, said rather than left implicit.** A block whose
figure is example syntax (`2GiB`, `512MiB`, `17GiB`) states no default and
has nothing to disagree with. A flag whose figure reaches the help only as
clap's `[default: …]` is already rendered from its constant. One figure is
out of reach by construction: the hub's `--still-there-every` quotes exit
**125**, which is `herdctl::NO_VERDICT_CODE`, and `herd-hub` does not
depend on `herdctl`.

**Evidence.** `herd-hub/src/main.rs`, `herd-runner/src/main.rs` and
`herdctl/src/main.rs`, each `the_long_help_states_every_figure_its_constants_decide`;
all three green on their first full run, and each demonstrated red by
changing one figure in one help string and restoring it to a byte-identical
sha256 (`history/2026-10-30-audit-tests.md`). Twenty-one pairings in all.
```

### Board row one — the sweeper's remaining untested entry point

```markdown
| ORCH-NEXT | **Audit one's F2 is narrower than stated, and what it got right is a *harness* gap rather than a one-clause gap.** F2 says the hub's startup sweep is kept off a part-written collection by one `||` and that *"nothing anywhere goes red"* if it is deleted. Measured on `store-guards`: deleting `|| !self.collection_is_whole(&id, &record)` (`store.rs:973`, moved from the audit's `:883` by `store-guards` itself) turns `store::tests::a_collection_still_arriving_is_never_swept` **red** on `could_not_free` (0 against 8) — a test the brief said did not exist — and **no byte is lost**, because `JobDirStore::evict` carries its own independent `collection_is_whole` guard at `:790`. Both clauses must go to lose a byte, and then the same test goes red on the bytes. What *was* true: all eight tests of the sweeper called `sweep_outputs()` on an already-open store, so the `recover` / `sweep_outputs` pair three lines apart inside `open_with` was exercised by nothing. `audit-tests` closes that one | `audit-tests`, 2026-10-30, re-measuring the audit's F2 | herd-step | done | **Settled, not open**, and written down so the severity is not re-inherited: F2's remaining residual is that `evict`'s guard is unreachable from the sweep while `:973` stands, so no test can distinguish them — defence in depth, correctly. The live gap F2 pointed at is closed by `store::tests::a_start_never_sweeps_a_collection_it_is_still_waiting_for` | — | `herd-hub/src/store.rs:973`, `:790`, `:590-593`; `store::tests::a_collection_still_arriving_is_never_swept`; `store::tests::a_start_never_sweeps_a_collection_it_is_still_waiting_for`; `history/2026-10-30-audit-tests.md` finding F-a |
```

### Board row two — the cross-crate figure in the hub's help

```markdown
| ORCH-NEXT+1 | **One figure in `herd-hub --help` has no gate reachable from where it is written.** `--still-there-every`'s doc comment explains itself with *"which `herdctl` reads as the stream ending with no verdict and reports as exit 125 on a job that succeeded"*. **125 is `herdctl::NO_VERDICT_CODE`** and `herd-hub` does not depend on `herdctl`, so the hub's new `--help` test cannot read it; every other figure in all three binaries' help now is read back through its own constant (D-NEXT) | `audit-tests`, 2026-10-30, while writing D-NEXT's hub test | herd-step | open | Two ways, and the cheap one first: **(a)** drop the figure from the sentence — it explains *why* the hub breathes on an idle link and the exact code is `herdctl`'s business, so *"reports as having no verdict"* says the same thing and is true whatever the code is; or **(b)** move the three exit codes into `herd-proto`, which both binaries already depend on, which is where invariant 4 would put a number two crates agree about. (a) is one line of prose; (b) touches a frozen surface and wants its own branch. Recommend (a), and (b) only if a third crate ever needs the codes | **D-NEXT** | `herd-hub/src/main.rs` `--still-there-every`; `herdctl/src/lib.rs:110`; `history/2026-10-30-audit-tests.md` finding F-d |
```

### Board row three — the rest of audit one's `NONE` column

```markdown
| ORCH-NEXT+2 | **Audit one's F3 to F6 are still untested, and F1 and F2 being closed is the argument for ranking them now.** `audit-tests` closed F1 (the real runner's barrier against a hub whose `readdir` fails) and the live half of F2 (the hub's startup sweep). The four below have no test in any harness: **F3/R2b**, a swept *collection* on a recreated state root — the only wrong-bytes failure mode on the audit's whole list, one job's output files written onto another job's record, with nothing in any log saying so; **F4/H1b**, the hub concluding from silence on an *open* socket that a named runner is gone, which `capabilities.rs:769-778` goes out of its way to avoid; **F5/H2a-H2b**, the watchdog's two checks, best covered by a test that cuts a real TCP relay against an in-process hub and whose deployed-threshold twin is `#[ignore]`d; **F6/R3**, the runner's 60 s `HUB_SILENCE_LIMIT`, asserted by nothing anywhere | audit one's F3-F6, 2026-10-30, carried forward by `audit-tests` | herd-step | owed | **F3 first**, and not because it is cheapest: it is the only one that writes wrong bytes rather than losing right ones, and its cheapest evidence is small — `killed_hub.rs:1313` already recreates a state root with real binaries on both sides, so it needs a spool planted under `outputs/` beside the verdict in `held-results/` and `.foreign-state-root` asserted on both. Then F4, which is the one branch of the lot with no test of any kind and whose fixture already exists (`owner_control.rs:519` sends `SIGSTOP`) | — | `…/scratchpad/2026-10-30-startup-sequence-audit.md` F3-F6 and its table rows R2b, R3, H1b, H2a, H2b; `history/2026-10-30-audit-tests.md` |
```

## Decisions needed

None. Everything here is a test, the two premises that turned out wrong are
corrected in findings with the measurements beside them, and the one
product-shaped question in the audits (F8, `pack-registry`'s
`LAYOUT_VERSION` 2 refusing the consumer's live state root) is not mine and
is already on the board.

## Resume point

The branch is pushed and is a test-only branch over `origin/store-guards`,
so it is ready for the batch. For whoever continues the audits' work:

```sh
# audit one's F3, the only wrong-bytes failure mode left on its list
git worktree add <path> -b foreign-swept-collection origin/main
# then: herd-hub/tests/killed_hub.rs:1313, plant a spool under
# outputs/{job}-{attempt} beside the verdict in held-results/ and assert
# .foreign-state-root on both. Real binaries both sides already.
```

## Processes

- Background `nix develop --command cargo …` shells and the processes the
  tests spawned, all started by me and all exited on their own: the five
  runs of test one each start one `herd-hub` and one `herd-runner` through
  `real_herd`, which kills and reaps them on drop. Nothing was killed by
  name; no `pkill`, no `killall`.
- The first consumer's `herd-hub` (3006543) and `herd-runner` (3021002)
  were untouched and are still up.
- One honest slip, reported rather than hidden: my first heavy phase ran
  `./scripts/bench-preflight.sh` and the build in one `;`-separated
  command, so the build started although the preflight had **REFUSED** on
  load 11.36 against a ceiling of 9.00. I noticed on reading the output,
  stopped the build with `TaskStop` (no stray `cargo` or `rustc` of mine
  survived, checked with `pgrep -af audit-tests`), and waited. Every later
  phase ran the preflight as its own command and on a cleared margin.
- `CARGO_TARGET_DIR` was `/mnt/ssd_980/herd/build/audit-tests`, nothing on
  the root filesystem, and it is removed now that the branch is pushed.
