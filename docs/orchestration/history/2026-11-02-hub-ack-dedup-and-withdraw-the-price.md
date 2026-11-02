# hub-ack-dedup-and-withdraw-the-price — handoff 2026-11-02

- **Item:** none allocated — briefed without an `ORCH` row, from the two OOM
  kills of the first consumer's hub on the night of 2026-11-02. One `board.md`
  row is drafted below, for the cost this change does **not** pay off
- **Agent:** herd-step
- **Branch / commit:** `hub-ack-dedup-and-withdraw-the-price` /
  `fd33002d5d4b51051420eda51fcba54f3da65caa` for all the code, plus this file
  as a prose-only commit on top of it; based on origin/main
  `846cde8dd2103127ea9a8f5126dbffd28fdd2a69`
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger` names
  the landing here, and fills the record of `templates/landing.md`
- **Not pushed.** The GREEN gate this branch holds is for `fd33002`, not for
  the tip that carries this file. See *Gates* and *Decisions needed*.

## Done

**Part 1 — `FromHub::JobFilesAck` is said once per (job, attempt) per
connection.**

`herd-hub/src/server.rs`, `relay_from_runner`, now carries
`let mut acked: HashSet<(JobId, u32)> = HashSet::new()` beside its
`AlreadySaid`, and the `Ok(ChunkLanded::Complete)` arm sends the frame and
prints `has every file of its collection` inside
`if acked.insert((job_id.clone(), attempt))`. Everything else in that arm is
unchanged and still unconditional: `clear_output_parts`, and
`the_collection_is_whole` — the second deliberately, because it is the belt
that releases a verdict held for a collection which completed before its
verdict landed, and `take_the_held_verdict` already makes it idempotent.

The set is a local in the frame loop, so it lives and dies with the link and
needs no eviction policy. That is the shape and the register of
`OwedAtJoin::CannotTell`'s `answered` in the same file, which the comment
names.

**Part 2 — the price paid at the verdict's `JobFilesHeld` is withdrawn.**

The paragraph that justified `44aaf1c` (2026-10-31, *Ask for a collection when
the verdict announces it*) by bounding its cost at *"`FILES_IN_FLIGHT` 8 times
`OUTPUT_CHUNK_BYTES` 1 MiB — 8 MiB, once per collection, self-correcting"* is
replaced. The replacement names what that semaphore does bound (chunk frames
queued **per connection**, across every collection on the link, hence bytes
resident in the runner, hence a share of 8/n with n collections in flight);
states that **nothing in it bounds the runner's cursor lead over this hub's
holdings**, the permit returning when the chunk reaches the kernel; says the
rewind is over the **whole collection**; names the field refutation with its
date; says the storm scales with the manifest and with concurrent collections;
and offers **no new number**, because the last number offered there was read
as a guarantee.

**The three facts the brief asked me to check myself, each re-verified in this
tree. `herd-runner/src/lib.rs` was not touched, doc comments included** —
`runner-resume-from-one-pass` is editing it concurrently:

- `FILES_IN_FLIGHT = 8` at `herd-runner/src/lib.rs:3743`; the field doc at
  `:1138` reads *"Per **connection**, like the link beside it"*; production
  builds one `Semaphore::new(config.files_in_flight)` per dial at `:4772`.
- `write_frames` returns the permit with `credit.add_permits(1)` after
  `sink.send(...)` (`:5111`-`:5127`), and its own doc says the permit is
  returned *"here and nowhere earlier, which is what makes
  [`Inner::files_credit`] a window on the socket rather than on the
  channel"*.
- `resume_from` (`:1039`-`:1051`) loops `self.manifest.iter().enumerate()` and
  rewrites every `self.sent[index]` from the hub's holdings — the whole
  collection, not the one file a frame is about.

**The sender count and the store's early return, also re-verified.** Outside
tests there is exactly one sender of `FromHub::JobFilesAck`, the arm above.
`write_output_chunk` early-returns `Ok(self.collection_state(id, &record))`
when `file_length(&whole) == Some(entry.bytes)`
(`herd-hub/src/store.rs:2554`-`:2556`); `collection_state` is `Complete`
whenever `collection_is_whole` holds (`:1293`-`:1299`); and
`collection_is_whole` is one `file_length` per manifest entry (`:706`-`:712`).

**`PROTOCOL_VERSION` is still 3** — `herd-proto/src/lib.rs:161`, re-read on the
committed tree. No frame shape changed; no field was added or removed; nothing
in `herd-proto` was edited.

**Tests.** `herd-hub/tests/spools_resumed.rs` gains
`a_whole_collection_is_acknowledged_once_per_link_and_never_not_at_all`, a real
hub over a real socket with a hand-driven peer, asserting four things:

1. the chunk that completes the collection **must** be acknowledged — the
   correctness boundary, whose failure message is *"a collection that just
   became whole must be acknowledged, or its runner can never free it"*;
2. `STRAGGLERS` (5) further copies of that same chunk draw **no** second
   acknowledgement, read against a health report and not against a clock: one
   link, one order, so anything the stragglers produced would be in the list
   ahead of the `HealthRecorded` that ends it;
3. the link is still alive at the end of that list, so the silence in (2) is
   not the silence of a dropped connection;
4. a **new** link, after the hub notices the old one is gone, offers the same
   whole collection and **is** acknowledged — the per-connection half, which
   is the sentence a swept spool is freed by (D19).

## Not done, and why

- **The detector was not proved against a known-bad tree.** The guard was
  neutralised with `|| true` and a single-test run was armed, but
  `bench-preflight.sh` refused for the whole window and the coordinator stood
  the branch down before it ran. So the new test is **green on the fixed tree
  and unmeasured on the unfixed one**: its silence is not yet evidence. The
  edit was reverted and the worktree is clean at `fd33002`. The run is one
  command, in *Resume point*.
- **The `collection_is_whole` walk is not paid off.** The store re-runs it for
  every straggler chunk, one `stat` per manifest entry, 662 of them for the
  collection that broke the consumer's hub. It happens before the arm this
  change guards, so a frame-level de-duplication cannot remove it; removing it
  needs the store, which the brief did not open. Named in the new comment and
  drafted as a board row below.
- **The unbounded outbox at `herd-hub/src/server.rs:3857` is untouched**
  (`mpsc::unbounded_channel`), as the brief directs: bounding it is a bigger
  change with a deadlock question, planned separately. This change removes one
  large producer into that channel; it does not bound it.
- **No `docs/decisions.md` entry**: the brief asked for none.
- **The attempt dimension of the key is not asserted by a test.** The key is
  `(JobId, u32)` because the runner's own `files_acknowledged` refuses to free
  one attempt's spool on another attempt's acknowledgement
  (`herd-runner/src/lib.rs:2759`-`:2763`). Arranging two attempts of one job,
  each with a whole collection, on one link was out of proportion to the risk,
  which is that the key is *narrower* than the behaviour being removed.

## Gates

**`./scripts/gate.sh` ran once, through Herd, on `fd33002` — the commit that
carries every line of code on this branch — and was GREEN.** Submitted as
`/mnt/ssd_980/herd/queue/bin/herdctl --hub ws://127.0.0.1:8791 run --label
build --memory-mib 4700 -- /bin/sh -c "cd <this worktree> && export
CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/hub-ack-dedup && ./scripts/gate.sh"`,
after `./scripts/bench-preflight.sh` said `ok, safe to start` on its ninth
attempt. Its verdict:

```
gate: GREEN sha=fd33002d5d4b51051420eda51fcba54f3da65caa steps=0-8:ok host=servernixos load1=16.66 load1-end=15.53 skips=under-load(flood-fifty=15.75>8,flood-two-fifty=15.75>8)
```

Log and receipt, in this worktree:

```
target/herd-gate/gate-20261102T085243Z-fd33002d5d4b.log
target/herd-gate/fd33002d5d4b51051420eda51fcba54f3da65caa.receipt
```

Steps, each `ok`: 0 the tree, sha and host; 1 `check-docs.sh`; 2
`check-orchestration.sh`; 3 `nix flake check`; 4 `cargo fmt`; 5 `cargo
clippy`; 6 the test suite; 7 the skips, both kinds; 8 the verdict and the
receipt.

**The new test and its neighbours, verbatim from step 6:**

```
     Running tests/spool_offer_answered.rs
running 1 test
test every_swept_spool_the_hub_acknowledges_is_deleted_and_none_set_aside ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 9.11s

     Running tests/spools_resumed.rs
running 3 tests
test a_hub_asks_a_rejoining_runner_for_a_collection_it_is_still_owed ... ok
test a_collection_the_hub_already_holds_is_never_asked_for_again ... ok
test a_whole_collection_is_acknowledged_once_per_link_and_never_not_at_all ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.13s
```

**Skips, as a list and never a count (D27).** Fifteen `HERD-SKIPPED` names
were observed and the gate's step 7 said *"the skip list matches the
expectation exactly"*; the `#[ignore]` list matched exactly at 38 names, each
carrying a reason. **This branch adds no skip of either kind**, so no file
under `scripts/expected-skips/` moves. Thirteen are the host's at-rest list:

- `a_ceiling_this_host_can_carry_is_proved_and_reported_with_its_device`
- `a_declared_ceiling_changes_what_the_inert_weight_line_says`
- `a_job_cannot_fork_past_its_own_pid_ceiling`
- `a_job_s_cgroup_carries_the_cpu_io_and_pid_bounds`
- `a_job_s_cgroup_carries_the_hard_io_ceiling_it_was_given`
- `a_job_s_record_carries_the_pressure_it_ran_under`
- `a_tighter_enclosing_memory_max_lowers_the_room_that_is_announced`
- `a_tmpfs_byte_a_job_writes_is_charged_to_its_own_memory_ceiling`
- `an_allocation_past_the_ceiling_kills_the_job_and_only_the_job` (the named
  memory outcome)
- `an_io_weight_that_cannot_bite_is_said_out_loud` (the weighted-device
  branch)
- `no_declared_ceiling_leaves_io_max_empty`
- `no_declared_quota_leaves_cpu_max_at_the_kernel_s_max`
- `the_runner_says_which_memory_ceiling_it_enforces` (the cgroup branch of the
  report)

and two are the load-gated benches, each excused out of **its own** reading
and not out of the gate's:

- `flood-fifty` — the bench read `15.75 > 8`, its own ceiling, and declined
- `flood-two-fifty` — the bench read `15.75 > 8`, its own ceiling, and
  declined

**`./scripts/check-docs.sh` was run again after this file was written, and
printed `docs ok`.** Its output is in the agent's report; nothing is added to
the tree afterwards.

**What has *not* been gated: the tip.** The repo's convention is that the gate
is the last thing run, after the handoff is committed — so a GREEN receipt
should exist for the tip and not for `fd33002`. It does not. The host sat
above `bench-preflight.sh`'s ceiling of 9.00 (load1 19.71 at the stand-down,
from the consumer's production, with nothing of ours in flight), and
scheduling the run is the coordinator's, not this agent's. The delta between
`fd33002` and the tip is this file alone: one markdown file under
`docs/orchestration/history/`, no Rust, no `Cargo.toml`, no script. **That is
a reason to believe the tip would gate green, not evidence that it did.**

## Risks and findings

1. **This is not the whole OOM.** The frame is one producer into an
   **unbounded** per-connection channel (`herd-hub/src/server.rs:3857`,
   `mpsc::unbounded_channel`), and the same storm still produces refused
   chunks, `JobFilesHeld` answers and `collection_is_whole` walks. 232 frames
   for four jobs is the measured share this change removes; nothing here
   measures what remains.
2. **A second flood this removes, read from the code and not measured.** Each
   repeat acknowledgement reached a runner that had already freed the
   collection, and `files_acknowledged` answers that case with a sentence on
   stderr (`herd-runner/src/lib.rs:2781`-`:2785`, *"holds no collection for
   it, so this acknowledgement frees nothing"*). So the flood was also writing
   that line to the runner's journal once per straggler. Not measured on the
   consumer's machine, and not measurable from here (D161).
3. **Two stale sentences corrected in test prose; no assertion changed
   anywhere.** `spool_offer_answered.rs` explained its diagnostic by *"the hub
   answers EVERY chunk of it `Complete` … that count is the number of chunks
   that reached the hub"*, which this change makes false; and
   `spools_resumed.rs`'s module doc said a chunk of an already-held collection
   *"is refused rather than written twice"*, which its own sibling test has
   refuted since D148. Both were reworded, and the module doc gained the third
   fact this branch adds. `spool_offer_answered.rs` still asserts
   `acknowledged_before == SWEPT` — one acknowledgement per collection as it
   *arrives*, which this change does not touch — and still turns on
   `set_aside.is_empty() && still_held.is_empty()`. What that file loses is
   diagnostic resolution: its `acknowledged_for_the_offer` figure used to say
   how far each pump got, and now says only whether each pump was answered.
   The new prose says that rather than pretending otherwise.
4. **A premise of the brief this branch did not verify, and did not need to.**
   The brief attributes the two OOM kills to this frame. The flood is measured
   — the consumer's own figures, quoted in the code comment — but the
   attribution of the kills to it is not something this branch measured, and
   their hub is not reachable from here (D161). The change stands on the flood
   alone.
5. **`spools_resumed.rs`'s module doc still says `PROTOCOL_VERSION` is "frozen
   at 2 (D73)"**, at its item 1. It is 3. Left alone: it is a statement about
   the constraint the startup sweep was designed under, not a claim about
   today, and rewriting it was outside what this change needed.
6. **The green test is not yet a detector.** See *Not done*: it has never been
   run against a tree without the guard, so its silence is not yet evidence
   that it would have caught this defect.

## Drafted for `herd-merger`

No `docs/decisions.md` entry — the brief asked for none.

One `board.md` row, for **Owed — agreed, not started**
with the columns `| ID | Item | Origin | Owner | State | Next action |
Depends on | Evidence |`, its id to be allocated at integration. Paste it
verbatim, replacing `ORCH-NEXT`:

| ORCH-NEXT | **A straggler chunk still costs the hub one `stat` per manifest entry — 662 of them for the collection that broke it.** De-duplicating `JobFilesAck` per link removed the *frame* and the log line; the walk behind them is the store's, and was not opened. `write_output_chunk` early-returns `self.collection_state(id, &record)` for a chunk of a file it already holds whole (`herd-hub/src/store.rs:2554`-`:2556`), and `collection_state` calls `collection_is_whole`, which is `file_length` on every entry of the manifest (`:706`-`:712`, `:1293`-`:1299`). So every late chunk of a 662-file collection walks 662 paths, and the number of late chunks scales with the manifest and with how many collections share the link's one `FILES_IN_FLIGHT` window — 61 arrived for one job inside one second on the first consumer's hub on 2026-11-02. **This is CPU and page cache, not the memory that OOM-killed them**, so it is owed rather than urgent, and it is a different question from bounding the hub's unbounded outbox (`herd-hub/src/server.rs:3857`), which is planned separately. The shape to price: whether the hub can answer a chunk for a collection it has already called whole without re-deriving wholeness. A per-link memory of which collections are complete exists now — `relay_from_runner`'s `acked` — but it lives in the server and not in the store, and D79's one-definition rule is what makes moving it a decision rather than a cleanup | `hub-ack-dedup-and-withdraw-the-price`, 2026-11-02, while de-duplicating the frame | herd-planner | owed | Price it against D79's one definition: say whether the store may be *told* a collection is whole, or whether the server may skip the store for a chunk it already knows is late, before either is written | nothing | `herd-hub/src/store.rs:2554`-`:2556`, `:706`-`:712`, `:1293`-`:1299`; `herd-hub/src/server.rs`'s `Ok(ChunkLanded::Complete)` arm and its comment; `history/2026-11-02-hub-ack-dedup-and-withdraw-the-price.md` |

## Decisions needed

**One, and it is the coordinator's, not Alexa's.** This branch is committed and
**not pushed**: the GREEN receipt is for `fd33002`, and the tip adds this
handoff on top of it. Either

- gate the tip on a quiet host and push on its GREEN — the convention, and the
  recommendation, since a receipt for the sha that is pushed is the thing the
  method asks for; or
- push as it stands, on the coordinator's word, taking the delta — one
  markdown file under `docs/orchestration/history/`, with `check-docs.sh`
  green on it — as prose-only and covered by the batch's own `gate.sh` at
  integration, which is `herd-merger`'s.

Nothing is lost by the first: the worktree and the warm build tree are kept.

## Resume point

Two commands, in this order, both on a host `bench-preflight.sh` has cleared,
and both the coordinator's to schedule:

1. **Prove the detector**, which is the one piece of evidence this branch
   lacks. Neutralise the guard — in `herd-hub/src/server.rs`, make the
   condition `if acked.insert((job_id.clone(), attempt)) || true {` — run
   `nix develop --command cargo test --test spools_resumed -- --nocapture`
   through Herd, and expect
   `a_whole_collection_is_acknowledged_once_per_link_and_never_not_at_all` to
   fail naming the straggler count. Then revert the edit and confirm the
   worktree is clean at the tip.
2. **Gate the tip** — `./scripts/gate.sh` through Herd, same submission as in
   *Gates* — and push the branch on its GREEN. No merge, no tag, no pull
   request: the landing is `herd-merger`'s.

## Processes

Three background shells, all started by this agent, all stopped by this agent
by task id and none by name:

- the preflight waiter (`wait-preflight.sh`), stopped when it was folded into
  the next one;
- `wait-then-gate.sh`, which polled `bench-preflight.sh` once a minute,
  submitted the gate on its ninth attempt and exited 0;
- `detector.sh`, armed behind the same preflight for the known-bad run and
  **stopped before it submitted anything**, at the coordinator's stand-down.

One Herd job ran: the gate above, on the queue's own hub
(`ws://127.0.0.1:8791`), `--label build --memory-mib 4700`. No `pkill`, no
kill by name, nothing on the consumer's instance.

**The build tree is kept, not deleted**, because the branch is not pushed and
the next run is a resumption of this one:
`/mnt/ssd_980/herd/build/hub-ack-dedup`, **14 GiB** on `/mnt/ssd_980` and not
on the root filesystem. It is deleted once the branch is pushed
(`workflow.md`, *Concurrency and host rules*; gotcha
`build-weight-off-root`). The worktree is kept for the same reason:
`<scratchpad>/wt-ack-dedup`, clean at `fd33002`.
