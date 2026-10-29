# operator-truth-quarantine — handoff 2026-10-29

- **Item:** ORCH-183
- **Agent:** herd-step
- **Branch / commit:** `operator-truth-quarantine` / see the push, based on
  origin/main `b37b2a4`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

**`docs/operating.md` — the operator's own page, made true.** Four places,
not the three the brief named.

1. *The disposable-container recipe, reason 5.* Was: "**It works when a hub
   would refuse to start at all.** One unparseable `job.json` makes a hub
   refuse the whole state root…" Now: "**It tells you what is in the root
   before any hub opens it.** `report` **names each record it cannot read,
   counts it, and goes on**…", with the refusal kept as a fact about
   **v0.4.1 and every release below it**. The earned sentence stayed; the
   claim about what a hub does is now scoped by version.
2. *§ A torn `job.json`…* retitled **"(v0.4.1 and below)"**, with "What
   happens, **on the hub you are running today**", and the chain re-dated
   from "the tree this page was written on" to "**Verified at v0.4.1**,
   which is the newest release and the newest build that does this".
3. *The `**Status**` paragraph is gone*, replaced by **What the fix
   changes** — the quarantine, what still refuses a root, a four-row
   version table in the same idiom as the spool defect below it, and "**It
   is on `main` and in no release.**"
4. *The reversed promise is stated as a reversal*, not quietly dropped:
   "**A `RECORD_VERSION` mismatch, because this page promised the
   opposite.** Until 2026-10-29 this section said [it] would deliberately
   **stay a hard error**… **That is not what was built.**"

**A fifth wrong place the brief did not name**, same false claim, found by
grepping `refuse` across the page: § *When a version refusal names two
figures* said a hub "**does not start** rather than guess" and printed the
`job.json has record version 1` message as an example of that. On `main` a
record-version mismatch does not stop the hub. Worse, and now written down:
a build whose `RECORD_VERSION` differs from the root's while its
`LAYOUT_VERSION` matches **starts and serves an empty queue**, every job
quarantined by name. D199 says **v1.0.0 is the release that moves those
numbers**, so this is a live v1.0.0 concern and not a hypothetical. The
section now separates the two numbers, keeps the layout refusal as the
rollback guard, and says "**Do not rely on the hub to stop you here.**"

**One honest limit added to the mitigation**, because the page tells the
operator to trust `report`: `report` and the hub do not parse a record the
same way, so "a clean `report` is evidence about damaged *bytes*, not a
promise that every record the hub will open is one it can use."

**`docs/remediation/13-crash-point-enumeration.md`** — a dated
**Amendment, 2026-10-29 (ORCH-183)** block after the Marks paragraph, and
an inline *superseded* marker at each named place: §0 point 1, §3's totals
block, §4.2's heading and opening, §5 point 1, and §8 **F** points 2, 3
and 5. Nothing below the amendment is rewritten — the document stands as
what was measured at `2ca6dc7`. The amendment carries the census the
harness asserts today, **observed by running it** (below), and notes that
the two cases the document names were renamed with the fix, giving both
old and new names so a grep finds something.

**`docs/remediation/README.md`** — row 42 (`13-…`) corrected to past tense
with the taken proposal and the new census; **and row 39 (`10-…`), which
the brief did not name**, carried the same claim in the present tense
("makes the hub refuse to start on the *whole* state root") and is
corrected the same way.

**`herd-hub/tests/durability.rs` — five tests, in the gate.** Every
fixture is a record `JobDirStore::create_job` wrote and the test then
damaged; no hand-written string. Each is armed against an intact root
first (`fn armed`), and the arming check was **proved to fire**, not
trusted.

| Test | Fixture | Arm it exercises |
| --- | --- | --- |
| `a_record_overwritten_with_non_utf8_bytes_quarantines_only_itself` | 4 bytes `f0 9f ff fe` over the middle of a real record | the read arm (`read_to_string` refuses) |
| `a_record_missing_a_required_field_is_quarantined_and_report_counts_it_read` | the real record, `state` removed, re-serialised | the **third** parse (`RecordFile`) |
| `a_record_replaced_by_a_directory_quarantines_only_that_job` | `job.json` unlinked, a directory `mkdir`'d in its place | the read arm (`EISDIR`) |
| `a_record_the_filesystem_will_not_hand_back_quarantines_only_that_job` | the real record moded `0o000` | the read arm (`EACCES`) |
| `a_torn_record_holding_the_highest_id_does_not_cost_that_id` | three minted, the **last** truncated to 2/3 | recovery's id high-water reading |

Test 2 also pins the `report`/hub divergence on the very same bytes:
`report.census.read == 1` and `census.unread` empty while the hub
quarantines, and a local `VersionProbe` mirror asserts the fixture still
parses with `record_version == RECORD_VERSION`, so the quarantine is
provably the record parse and not the version arm.

Test 4 skips loudly under D27 when `geteuid()` is root, via
`herd_runner::testing::skipped`; it **ran for real** on this host (euid
non-zero) and is not among the suite's 12 skips.

## Not done, and why

- **`herd-hub/src/` untouched**, as the brief required. `git status` shows
  exactly the four allowed paths plus this handoff.
- **`docs/decisions.md` and `docs/orchestration/board.md` untouched.** The
  two drafted items are under *Decisions needed*, for `herd-merger`.
- **No `#[ignore]` removed from `crash_points.rs`.** The brief forbade it;
  the enumeration was run on purpose instead.
- **No release cut, no tag, no merge, no PR, no write to `main`.**

## Gates

Run with `CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/operator-truth-v10`,
off the worktree and off the root filesystem.

`nix develop --command cargo fmt --all --check` — exit 0, no output.

`nix develop --command cargo clippy --workspace --all-targets -- -D warnings`:

```
    Checking herd-hub v0.4.1 (…/operator-truth-v10/wt/herd-hub)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.74s
```

`nix develop --command ./scripts/test.sh`:

```
==============================================================
 12 assertion(s) COULD NOT RUN on this host and were skipped.
 A green run does NOT mean these were checked.
==============================================================
test suite: PASSED
```

All five new tests appear as `ok` in that run. The 12 skips are the
host's standing cgroup-delegation skips; **none is one of the five**
(`grep HERD-SKIPPED | grep -c` over their names = 0).

`nix flake check` — `all checks passed!` (omitted `aarch64-linux`, as
always on this host).

The ignored enumeration, run on purpose and not altered:

```sh
nix develop --command cargo test -p herd-hub --test crash_points \
  -- --ignored --nocapture --test-threads=1
```

```
308 states built
  0 refuse the whole state root at `JobDirStore::open`
  176 open with the damaged record quarantined by name (V10)
  44 open with the job silently gone from the queue
  4 leave `events.jsonl` AHEAD of `job.json` (D103's one rule)
  231 leave a stale temporary that nothing removes

test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 10.40s
```

**The last run**, on the committed tree, after this file was committed:
`./scripts/check-docs.sh`. Its output goes to the agent's report, not into
the tree it gates.

**Skips:** 12 in `./scripts/test.sh`, all pre-existing and all about
cgroup v2 delegation this host does not grant. None belongs to this task.
Test 4 carries a D27 skip for root and did not take it here.

## Risks and findings

1. **The fix is in no release, and the brief's framing was the wrong way
   round.** The tree is `0.4.1` and the newest tag is `v0.4.1`, so **every
   released version still has the refusal**. The brief said an operator
   acting on the page "does unnecessary work and may hold back a healthy
   upgrade"; for this defect the opposite holds — an operator on v0.4.1
   still has it, and a page that simply declared it fixed would have been
   a second false statement. The page is scoped by version instead.
2. **`RECORD_VERSION`-only move now starts a hub on a wholly quarantined
   root.** Verified by reading `store.rs`: the layout check is in
   `open_with` and returns `Err`; the record-version check is in
   `read_stored` and returns `Ok(Stored::Quarantined(…))`. So with
   `RECORD_VERSION` bumped and `LAYOUT_VERSION` unchanged, every record in
   an existing root is quarantined, the hub binds its port and serves an
   empty queue. Loud (a line per record plus a count), but not a refusal.
   **D199 names v1.0.0 as the release that moves these numbers.** This
   wants a decision before v1.0.0, not after.
3. **The brief's premise for test 5 is refuted by the landed code, and the
   test was restructured because of it.** `recover` takes
   `ids_minted.max(highest_on_disk)`. On any root this build wrote,
   `store.json`'s mark alone holds the id line, so renaming a quarantined
   record's directory would **not** have made the single-arm test red for
   the reason the brief gives. It is load-bearing only on a root with no
   mark — and `store.rs`'s own comment calls that set "every root in
   existence", the first consumer's included. The test now has two arms,
   the second on a root whose `ids_minted` the test strips, and that is
   the one that catches a rename.
4. **Both detectors were proved to fire, not trusted for being silent.**
   (a) Moving `armed()` to *after* the damage in test 1 → red at
   `durability.rs:927` with the arming message. (b) Renaming the
   quarantined directory to `j0000000003.bad` inside
   `torn_highest_of_three` → red, with
   `herd-hub: ignoring …/jobs/j0000000003.bad, which is not a job directory`
   on the log and the assertion message naming the rename. Both mutations
   were reverted; `grep -c MUTATION` over the committed file = 0.
5. **D218 already records more than the brief thought.** Its *Choice*
   explicitly covers "a record… **of a record version this build does not
   read**", so the version arm's behaviour **is** recorded. What is not
   recorded is that this **reverses** `13-…md` §5 point 1's published
   caveat and the promise `docs/operating.md` carried to the operator.
   The drafted entry below is narrowed accordingly. D218 also already
   carries a *Residue* line naming §4.2 and `README.md:42` as ORCH-183 —
   this task was foreseen.
6. **A planner mislabel, harmless but worth saying:** the brief's "§5
   point 5 (~477)" is §**8**'s **F** list point 5. §5's own numbered list
   has three points, and its point 1 is the `RECORD_VERSION` caveat. Both
   are corrected.
7. **A census figure the brief did not mention also moved:** stale
   temporaries went **99 → 231**, because every record shape now opens, so
   three shapes of temporary are reached across seven record shapes rather
   than three. The amendment says so.
8. **The cgroup floor was lifted on my own reading, disclosed.** See
   *Processes*.

## Decisions needed

**Drafted for `herd-merger` — `docs/decisions.md`, a new entry headed
`D-NEXT` (the merger allocates the number):**

```markdown
## 2026-10-29 — D-NEXT The RECORD_VERSION arm quarantines too, not refuses

**Context.** D218 put a record the hub cannot read on D52's quarantine
path, and its *Choice* includes a record "of a record version this build
does not read". Two documents had published the opposite promise and
neither was corrected with it: `docs/remediation/13-crash-point-enumeration.md`
§5 point 1 argued the version arm "should probably stay an error", and
`docs/operating.md` — the page shipped to the first consumer's operator,
who nobody here may reach (D161, invariant 9) — told them the mismatch
"stays a hard error, because damage and a wrong binary are different
failures". D218 recorded the choice and not the reversal.

**Choice.** The reversal stands, and is recorded here rather than left in
a superseded dossier. **The downgrade guard is `LAYOUT_VERSION`**, read
once out of `store.json` in `open_with`, which still refuses a whole root
by name. The per-record `RECORD_VERSION` arm quarantines, per record.

**What this costs, and it is not nothing.** A build whose `RECORD_VERSION`
differs from the root's while its `LAYOUT_VERSION` matches now **starts
and serves an empty queue**: every record is quarantined, each named with
its path, with the count on the line that says the hub is listening. That
is loud but it is not a refusal, and **D199 names v1.0.0 as the release
that moves these numbers**. Either v1.0.0 moves `LAYOUT_VERSION` with
`RECORD_VERSION` — which the four-step procedure in `docs/operating.md`
already assumes — or that release needs a guard of its own.

**Alternative rejected.** Returning the version arm to a hard error. It
would put back the defect D218 removed for the one record shape an
operator is least able to diagnose, and `LAYOUT_VERSION` already carries
the format contract at the only granularity where refusing is right: the
whole root.

**Evidence.** `herd-hub/src/store.rs` (`open_with`'s layout check against
`read_stored`'s version arm); `herd-hub/tests/durability.rs`;
`docs/operating.md` §*When a version refusal names two figures* and
§*A torn `job.json`…*; `docs/remediation/13-crash-point-enumeration.md`
amendment; **D218**, **D52**, **D199**; ORCH-183.
```

**Drafted for `herd-merger` — `docs/orchestration/board.md`, a new row:**

> **ORCH-nnn — `report` is the laxer reader, and says nothing about it.**
> `herd-hub report` parses `RecordOutputs` (`record_version` and `outputs`,
> both `#[serde(default)]`) while the hub parses the whole `RecordFile`, so
> a record missing a field the hub requires is counted in `census.read` by
> `report` and **quarantined** by the hub. The page tells the operator to
> read the root with `report` before starting a hub on it, so the two
> numbers can disagree exactly where it is acted on. Pinned by
> `a_record_missing_a_required_field_is_quarantined_and_report_counts_it_read`
> in `herd-hub/tests/durability.rs`, and stated on `docs/operating.md`.
> Decide whether `report` should also report what the hub would quarantine.
> Found: 2026-10-29, ORCH-183.

**A second board row I am proposing beyond the brief**, because finding 2
is a v1.0.0 blocker shaped thing and the orchestrator asked to hear what
the code refutes:

> **ORCH-nnn — a `RECORD_VERSION`-only move starts a hub on a wholly
> quarantined root.** The layout check refuses the root; the record-version
> check quarantines per record. So a release that moves `RECORD_VERSION`
> and not `LAYOUT_VERSION` starts, binds its port and serves an empty
> queue. **D199 names v1.0.0 as the release that moves them.** Decide
> before v1.0.0 whether the two numbers move together or the release needs
> its own guard. Found: 2026-10-29, ORCH-183.

## Resume point

Nothing is pending on this branch. For `herd-merger`, at integration:
take the two drafted items above into `docs/decisions.md` (as `D-NEXT`)
and `docs/orchestration/board.md`, and include this branch in batch 9's
`./scripts/gate.sh` run.

## Processes

- **No PID was started that outlived a command.** Two short background
  waiters on `kill -0 465373` (the merger's `cargo test --workspace
  --no-fail-fast`), both exited on their own when it did. No signal was
  sent to any process. **PID 3046930 and port 18461 untouched**; the
  consumer's live `herd-runner` (PID 3021002) untouched. No `pkill`, no
  `killall`, nothing killed by name.
- **One suite at a time.** `./scripts/bench-preflight.sh` refused at
  21:33Z on the load ceiling (9.86 ≥ 9.00) with the merger's suite
  genuinely running; the phase waited for PID 465373 to exit rather than
  running beside it.
- **The cgroup floor was then lifted on my own reading, with figures.**
  After the merger's suite exited, the preflight still refused on the
  slice floor alone, with load 2.34 and **no** `cargo`/`rustc` running.
  Read from `/sys/fs/cgroup/chatons.slice` (read only; nothing written
  under `/sys/fs/cgroup`, no cache dropped): `memory.high` 18 432 MiB,
  `memory.current` 12 759 MiB, of which **`file` 11 480 MiB
  (reclaimable page cache, left by the suite that had just run)**,
  **`anon` only 692 MiB**, `kernel` 588 MiB. Room counting cache
  5 673 MiB — what the preflight measures — against **17 152 MiB
  ignoring cache**, which is what a build can actually have. I therefore
  re-ran with `HERD_PREFLIGHT_MIN_CGROUP_ROOM_GIB=5` (below the measured
  5 673 MiB), got `bench-preflight: ok, safe to start`, and ran the
  suite. The suite then passed. **The floor's default was not changed and
  no file was edited to lift it.**
- **`CARGO_TARGET_DIR` was `/mnt/ssd_980/herd/build/operator-truth-v10`**,
  outside the worktree and off the root filesystem. It is removed once
  the branch is pushed; the report says so.
