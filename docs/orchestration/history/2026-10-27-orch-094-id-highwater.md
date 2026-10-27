# orch-094-id-highwater — handoff 2026-10-27

- **Item:** ORCH-094
- **Agent:** herd-step
- **Branch / commit:** `orch-094-id-highwater` / `1ac89d1`, based on
  origin/main `cd59ef5`
- **PR:** #97, opened and **not merged**
- **Worktree:**

```text
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/orch-094/wt
```

## Done

The hub now keeps a **high-water mark of job ids** that does not depend on
what is left in `jobs/`.

**Where it lives.** `ids_minted: u64` in `store.json`
(`StoreFile`), beside `state_root_id`, with `#[serde(default)]`.
**Not** in `index/state.json`, which is what `docs/disk-policy.md` §7
proposed. The reason is the shape of the two files: `index/state.json` is
derived, and every path in `usable_index` that doubts it throws it away
and reads the records instead — right for a summary of what the
directories already say, fatal for a number that *nothing* on the disk
can reconstruct once those directories are gone. `store.json` is the
root's authoritative identity file; it is rewritten at every start, an
unreadable one already refuses the start, and it is the natural companion
of `state_root_id`: that field says *which* root a `j…` belongs to, this
one says that a single root never hands the same `j…` out twice.

**How it is made durable, and which way it fails.** Through
`durable::write_file` (D8): temporary, `fsync`, rename, directory
`fsync` — so a reader sees the whole old file or the whole new one, and
the value survives the machine, not merely the process. In
`JobDirStore::mint` the mark is raised **before** the `create_dir_exclusive`
that claims the id, one atomic write per mint beside the seven that
function already pays. The only thing a kill in that window can cost is a
**gap in the ids**; it can never cost an id handed out twice. D148 already
says a number is an identifier and gaps stay.

**Migration**, for a state root that has no mark — which is every root in
existence, the consumer's running hub included:

- **Field absent** → `serde(default)` reads 0, `recover` takes the mark
  from the highest surviving job directory, **writes it at that first
  open** and says so on stderr. Written at the open and not at the first
  mint on purpose: an operator who empties `jobs/` between the upgrade and
  the next job would otherwise undo the upgrade, which is the defect
  itself.
- **Field lower than a surviving directory** (a `store.json` restored from
  an older copy than `jobs/`) → the **higher wins**, and it is written
  back. Not a tie-break: each of the two is *evidence that an id was handed
  out*, and that evidence only accumulates. A directory proves its own id
  whatever the mark says; the mark proves ids were spent whatever is left
  in `jobs/`. Taking the lower would re-issue an id one of them has already
  seen.
- **Field higher than the directories** → the mark wins and nothing is
  written. This is the ordinary post-fix state: a crash between the mark
  and the `mkdir`, or a `jobs/` records have been removed from.
- **Field garbage** (a string, a float, a negative) → `StoreFile` does not
  parse, so `open_with` refuses to start naming `store.json`, which is the
  behaviour an unreadable `store.json` already had. This is deliberate and
  it is the one place the blast radius widens: of everything in a state
  root the mark is the only thing no reading of the disk can reconstruct,
  so a hub that cannot read it cannot promise an id is fresh. It says so
  instead of guessing low.

**`RECORD_VERSION` and `PROTOCOL_VERSION` are untouched.** 1 and 2, as
D73 froze them. Nothing in this change goes near either: `StoreFile` has
no `deny_unknown_fields`, so an older hub reading this `store.json` ignores
`ids_minted` and behaves exactly as it does today. `LAYOUT_VERSION` is
also unchanged at 1 — it governs the directory layout, and no directory
moved.

**`usable_index`.** No interaction, by construction. The mark is not in
`index/state.json`, `IndexFile` is unchanged, and `index.jobs_accounted`
still counts job directories and nothing else. A mark far above the
directory count therefore cannot make the hub throw the index away, which
is what would otherwise happen at every start once anything removes a
record. The test
`a_mark_far_above_the_directory_count_is_not_the_index_s_business`
asserts that the index file carries no `ids_minted` and still accounts for
2 directories with the mark at 10 000.

### What the six new tests in `store::tests` assert

1. `the_next_id_is_above_every_id_this_root_has_ever_minted` — the
   headline. Three ids minted, **every job directory and the index
   removed**, the root reopened: the next id is above 3. This is the test
   that is red on `cd59ef5`.
2. `an_id_is_marked_spent_before_the_directory_that_claims_it` — after a
   mint, `store.json` already names that id as spent (the ordering inside
   `mint`); then the post-crash state is written by hand — mark raised
   four past the last directory, no directory for any of the four — and
   the next mint comes out at mark + 1. A gap, never a reuse.
3. `a_root_with_no_mark_takes_one_from_its_highest_job_directory` — the
   upgrade. `ids_minted` is deleted from `store.json` to make the root
   look like every existing one; the next open writes the mark as 2; the
   directories are then removed and the next id is 3.
4. `a_mark_behind_the_directories_loses_to_them` — mark forced to 1 with
   `j0000000003` on the disk: the open raises the mark back to 3 **on the
   disk** and the next id is 4.
5. `a_mark_that_cannot_be_read_stops_the_hub_naming_the_file` —
   `"ids_minted": "lots"` makes the open fail with `store.json` in the
   message.
6. `a_mark_far_above_the_directory_count_is_not_the_index_s_business` —
   as above.

## Not done, and why

- **Retention, record removal, the sweeper: untouched.** Out of scope by
  the brief. Nothing in this branch removes anything.
- **No entry in `docs/decisions.md`.** The brief did not ask for a
  `D-NEXT`, and only `herd-merger` appends. The choice of `store.json`
  over `IndexFile`, and the choice to refuse the start on an unreadable
  mark, are the two things worth a numbered decision if the orchestrator
  wants one; they are stated here and in the PR body meanwhile.
- **`docs/disk-policy.md` is not updated** to say the mark landed in
  `store.json` rather than `IndexFile`: that document is PR #94, not yet
  merged, and editing a file that only exists on another branch would
  collide.
- **No measurement of the extra write.** One atomic write per mint is
  added. It was not benched; `11-sync-prices.md` prices an ext4 flush at
  ~989 µs and the mint path already pays seven.

## Gates

All four Rust gates plus `check-docs.sh` were run, in this worktree,
through `nix develop --command`.

```text
$ nix develop --command cargo build --workspace
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 9.07s
   (exit 0)

$ nix develop --command cargo fmt --all --check
   (no output, exit 0)

$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 13.14s
   (exit 0)

$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
   (exit 0)
```

`nix develop --command ./scripts/test.sh` — **RED on this host, for a host
reason**, exit 101. The cleanest of the runs (load average 5.3 at the
start) failed exactly two targets:

```text
error: 2 targets failed:
    `-p herd-hub --test admission`
    `-p herd-hub --test slots`

test a_killed_hub_reads_the_reservations_back_off_the_records ... FAILED
test the_load_is_on_the_placement_line_and_stops_nothing ... FAILED
test the_derived_j_is_recorded_and_is_what_the_job_is_given ... FAILED
```

That is **ORCH-069**, named in the brief: these compare a figure derived
from live memory against constants written for a 17 GiB machine. The hub's
own words in the failure say it — *"lender offers 1780 MiB and 4
threads"*, *"lender admits at most 756 MiB of memory (1.7 GiB lent, 1 GiB
kept as its own floor) and this job needs 4 GiB"* — and the `slots` case
asks for `--build-thread-mib 1792` on a runner offering 1780 MiB. Not
mine, and **not reported green**.

Verified against the base: `cd59ef5` was stashed into this same worktree
and those two targets run on it alone. `admission` came back **0 passed, 5
failed** and `slots` **1 passed, 7 failed** — redder than with this change,
because the host was busier at that moment. Both are red on the base.

The new unit tests all pass in the same run:

```text
test store::tests::the_next_id_is_above_every_id_this_root_has_ever_minted ... ok
test store::tests::an_id_is_marked_spent_before_the_directory_that_claims_it ... ok
test store::tests::a_root_with_no_mark_takes_one_from_its_highest_job_directory ... ok
test store::tests::a_mark_behind_the_directories_loses_to_them ... ok
test store::tests::a_mark_that_cannot_be_read_stops_the_hub_naming_the_file ... ok
test store::tests::a_mark_far_above_the_directory_count_is_not_the_index_s_business ... ok

test result: ok. 184 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
   (herd-hub lib; 178 before this branch)
```

**Skips (D27): at least 12, each printed as a skip and none counted as a
pass.** All twelve are the cgroup-v2-delegation family of
`herd-runner --test confinement`; the suite's own banner says the list is
a floor and not a count, because the suite did not finish:

```text
HERD-SKIPPED: a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device
HERD-SKIPPED: a_declared_ceiling_changes_what_the_inert_weight_line_says
HERD-SKIPPED: a_job_cannot_fork_past_its_own_pid_ceiling
HERD-SKIPPED: a_job_s_cgroup_carries_the_cpu_io_and_pid_bounds
HERD-SKIPPED: a_job_s_cgroup_carries_the_hard_io_ceiling_it_was_given
HERD-SKIPPED: a_job_s_record_carries_the_pressure_it_ran_under
HERD-SKIPPED: an_allocation_past_the_ceiling_kills_the_job_and_only_the_job
HERD-SKIPPED: an_io_weight_that_cannot_bite_is_said_out_loud
HERD-SKIPPED: a_tighter_enclosing_memory_max_lowers_the_room_that_is_announced
HERD-SKIPPED: no_declared_ceiling_leaves_io_max_empty
HERD-SKIPPED: no_declared_quota_leaves_cpu_max_at_the_kernel_s_max
HERD-SKIPPED: the_runner_says_which_memory_ceiling_it_enforces
```

An earlier run of the same suite, under heavier load, failed six targets
and 29 tests, 17 of them at `real_herd/mod.rs:649` — *"herdctl never came
back"*, the `PATIENCE` timeout. Each of those targets passes on its own
(`killed_hub` 18/18, `approval` 5/5, `follower_alive` 4/4). That is
contention on a shared host, not a defect of this branch, and it is why
the figures above are from the quieter run.

## Risks and findings

- **One extra durable write per mint.** The mark is written through the
  atomic-rename path, so a mint now costs eight such writes instead of
  seven. Measured at nothing; if ORCH-083 to ORCH-085 tier the durability
  of the store, this write belongs in the **decisive** tier and must not
  be moved out of it — it is the one write whose loss re-issues an id.
- **`store.json` stopped being near-immutable.** It is now rewritten on
  every mint. `herd-hub/tests/real_herd/mod.rs:715` reads it concurrently
  with a running hub; the atomic rename means that read always sees a
  whole file, and the suite is green on that target. Anybody who later
  reads `store.json` with a non-atomic writer will break this.
- **An unreadable `store.json` is now fatal for one more reason.** It was
  already fatal when it did not parse; a garbage `ids_minted` is a new way
  to reach that. An operator's recovery is to fix or delete the field,
  which the message does not spell out — it names the file and the parse
  error. If that is judged too blunt, the alternative is to fall back to
  the directories with a loud line, and it is a decision, not a tweak:
  falling back means silently re-issuing ids.
- **Diff overlap.** This touches `herd-hub/src/store.rs` in four places:
  `StoreFile`, `Live::next_sequence`'s doc, `open_with`, and
  `recover`/`mint`. ORCH-079 and ORCH-080 add test files only, so they
  should not collide; PR #85 and anything else inside `mint` or `recover`
  will.
- **Board id collision.** PR #94 (`docs/disk-policy.md` §7) proposes its
  own **ORCH-094** row. If both land, `check-docs.sh` fails on "uses an id
  twice". The merger takes one of the two; this branch's row is in
  `board.md` under *Owed*, state `done`.
- **The planner's diagnosis was right.** `store.rs:879` was
  `sequences.last().copied().unwrap_or(0) + 1` and `IndexFile` carried no
  mark; I found nothing that refutes it. Only its *placement* proposal
  ("in `IndexFile`, or beside it") is departed from, for the reason given
  above — and "beside it" is arguably what this is.

## Decisions needed

1. **Does the choice of `store.json` over `index/state.json` want a
   numbered decision?** *Recommendation:* yes, one short entry — the rule
   it encodes ("a number that cannot be re-derived never lives in a
   derived file") will be reached for again by M12. *Fallback:* leave it
   as the doc comment on `StoreFile::ids_minted`, which states it in full.
2. **Is "refuse to start on an unreadable mark" the right failure?**
   *Recommendation:* yes, and it needs no decision because it is the
   existing behaviour of an unparseable `store.json`. *Fallback:* fall
   back to the directories with a warning, which trades a refusal for a
   silent id reuse — not recommended.

## Resume point

The next task on this thread is **ORCH-098** (M12 record retention), which
the board lists as depending on ORCH-094 and on Alexa's answer to
`docs/disk-policy.md` §6 C. Nothing in this branch has to be finished
first. To pick this branch back up:

```sh
cd /mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/orch-094/wt
nix develop --command cargo test -p herd-hub --lib store::
```

## Processes

No long-running process was started. Every `cargo` and `nix develop`
invocation ran in the foreground in this worktree and exited before the
next; the test suite's hubs and runners are children of those and were
gone with them. Nothing was killed by name. The worktree's `target/` is
removed after the push.
