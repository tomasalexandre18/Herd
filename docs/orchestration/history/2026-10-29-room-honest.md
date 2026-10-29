# room-honest — handoff 2026-10-29

- **Item:** ORCH-198 (the `--help` half) plus the runner's room arithmetic
- **Agent:** herd-step
- **Branch / commit:** `room-honest` / see the report, based on origin/main
  `b37b2a4` ("Land batch 8: thirteen branches, from the preflight to
  the sweep")
- **Landing:** none — an agent pushes its branch and lands nothing (one
  landing per batch, `docs/orchestration/workflow.md`); `herd-merger`
  names the landing here, and fills the record of `templates/landing.md`

## Done

### 1. A runner lends what its host can really lend

`herd-runner/src/room.rs` subtracted the enclosing cgroup's `memory.current`
whole from its ceiling. `memory.current` counts reclaimable page cache, so a
runner on a machine that had merely *read* files announced almost nothing, and
the sentence `Enclosure::why()` prints about itself was false. E-22 is a merged
`must` promising `MemTotal` unless the owner says otherwise.

**The rule now implemented:** the charge is subtracted **less the reclaimable
file pages**, and the reclaimable figure is `inactive_file` **less `shmem`**,
both out of `memory.stat`.

- `inactive_file`, not `file`: those are the pages the kernel drops first,
  while `active_file` has been touched more than once and may be in use.
  **`active_file` is deliberately still counted as charged.** The result is a
  *floor* on the room, not an estimate of it — erring towards less room is the
  only direction invariant 2 allows.
- `shmem` comes back off, which is one step past the brief and in the same
  direction. tmpfs and `shm` pages sit on the file LRUs and are counted inside
  `inactive_file`, but they are swap-backed and cannot be dropped — and a
  runner's own job workspaces live on a tmpfs (D79), so on a runner host this
  is not hypothetical. Where `shmem` is 0, as on this host, it changes nothing.
- An **unreadable `memory.current`** is still read as the ceiling being fully
  charged, and now reclaims **nothing** against that assumption: subtracting
  cache from a worst case would turn a safe guess into a generous one.
- An unreadable or fieldless `memory.stat` reclaims 0, which is exactly the
  figure Herd announced before this change — pessimistic, never generous.

`Enclosure` gains `reclaimable_bytes` and `held_bytes()`; `room_mib()` is
`ceiling - held`. `why()` now says "… already holds X **of which Y is
reclaimable page cache** …", so the sentence is true again.

### 2. `scripts/bench-preflight.sh` moved with it

Its `:35` comment says it deliberately mirrors `room.rs`, and it still does:
the shell gains `reclaimable_in()` beside `numeric_in()`, `cgroup_enclosure`
carries a fifth field, check 4 subtracts `held` rather than `charged`, and it
prints a second line naming the charge and how much of it is cache. The
mirroring paragraphs were rewritten to state the subtraction and the knob.

### 3. The `--help` that was wrong by 32×

Only the `--job-output-cap` help text in `herd-runner/src/main.rs`. The code
that settles it: **`herd-runner/src/confine.rs:82`,
`pub const DEFAULT_OUTPUT_CAP: u64 = 1 << 30`** — 1 GiB. The old text said
"32 MiB by default, which is also the most this can be … the flag only ever
lowers it. A higher value is refused at startup". Three things wrong: the
figure (32×), the direction (the flag may now raise it), and the consequence
(a cap above `--job-disk-budget` is **clamped** at startup, `confine.rs:391`,
not refused). The 32 MiB figure migrated to a different concept:
`herd_proto::INLINE_OUTPUT_MAX` at `herd-proto/src/outputs.rs:99` is
`32 << 20`, the inline/streamed threshold. `confine.rs:73-81` was right and
says so itself; `main.rs` had not been updated with it.

## Not done, and why

- **`herd-runner/src/main.rs:474-475`**, the comment "A cap past what a frame
  can carry is refused by `Confined::prepare` with every other unusable
  figure, so there is nothing to check here", is **stale in the same way** and
  left untouched: it is a comment, not the `--job-output-cap` help text the
  brief scoped me to. `Confined::prepare` clamps against the disk budget and
  no longer refuses anything frame-shaped. A finding, not an edit.
- No entry written to `docs/decisions.md` or `docs/orchestration/board.md`;
  a draft entry is below for `herd-merger`.
- Nothing touched in `lib.rs`, `sweep.rs`, `outputs.rs`, `size.rs` or
  `tests/confinement.rs`. `tests/confinement.rs:1432-1512` reads `Enclosure`
  through `enclosing()` and never constructs one, so the new field needs no
  change there and none was made.

## Gates

All four Rust gates plus `check-docs.sh`, run on this tree before this file
was committed, from
`CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/room-honest` under
`nix develop --command`. Verbatim in the agent's report. Verdicts:

- `cargo fmt --all --check` — **exit 0**, clean (one diff found on the first
  pass, `cargo fmt --all` applied, re-checked clean).
- `cargo build --workspace` — **exit 0**, `Finished dev profile in 24.60s`.
- `cargo clippy --workspace --all-targets -- -D warnings` — **exit 0**.
- `./scripts/test.sh` — **exit 0**, `test suite: PASSED`, 230 + 204 + 69 + 68
  + 39 + … passed, 0 failed, with **14 HERD-SKIPPED assertions**, copied as
  skips and counted as no pass (D27).
  - **Twelve** are the pre-existing cgroup-delegation skips of
    `tests/confinement.rs`, every one of them reading "cgroup v2 delegation is
    unavailable: this runner cannot create a cgroup under
    `/sys/fs/cgroup/chatons.slice/chatons-herd-claude-256608.scope`
    (Permission denied (os error 13))". They are named verbatim in the agent's
    report. One of them,
    **`a_tighter_enclosing_memory_max_lowers_the_room_that_is_announced`**, is
    the real-cgroup half of the very code this branch changes and **did not
    run here**: it needs `systemd-run --user --scope -p Delegate=yes`. So the
    new arithmetic is proved against fixtures and against one read of this
    host, never against a cgroup Herd made itself.
  - **Two** are `flood-fifty` and `flood-two-fifty`, the load-gated flood
    benches, skipped because the one-minute load was over their ceiling of 8
    while the suite ran. That is the one shape
    `scripts/expected-skips/local-under-load.txt` excuses by name, and the
    gate judged them and still printed PASSED. **Their two reason lines were
    not captured verbatim** — only their names and the count. A narrowed
    re-run, `./scripts/test.sh flood` at load 7.64, **ran** both instead of
    skipping them, which is that load dependence itself.
  - An earlier full run of the same code, at a quieter moment, had **12**
    skips and no flood skip.
- `./scripts/check-docs.sh` — **exit 0**, `docs ok` (plus its standing note
  that D170 is a gap, which gaps stay, D148).

**The last run, on the committed tree, is `./scripts/check-docs.sh`**; its
output goes to the agent's report, not into this tree.

## Risks and findings

- **The brief's 3224 MiB is real but was the bad end of a range, not a
  constant.** Measured at 22:17 by the orchestrator (not by me): charge
  14.85 GiB, `inactive_file` 8.18 GiB. My own readings at 22:35:51 CET on
  `/chatons.slice`: `memory.high` 19327352832, `memory.current` 9308774400,
  `anon` 545603584, `inactive_file` 2684776448, `active_file` 5517914112,
  `shmem` 0, `MemAvailable` 22786864 kB. The host's cache had largely gone
  from inactive to active between the two readings, so **before 9548 MiB,
  after 12107 MiB** — a gain of 2.5 GiB, not 8. Nine minutes later the cache
  had swung back and the gain was 7 GiB again (next item). Both comments now
  carry the range rather than one figure. The gain is smallest exactly when
  the cache is mostly active, which is the conservative rule working rather
  than failing.
- **The gate-stability premise holds, but only at some readings.** At 22:35
  `MIN_CGROUP_ROOM_GIB=8` would **not** have refused this host even before the
  change (9548 MiB > 8192 MiB). At **22:44:53 CET** it would, and the two
  scripts run back to back prove the fix end to end on this host:
  `memory.current` 16066482176, `anon` 605233152, `inactive_file` 7482187776,
  `active_file` 7277080576, `shmem` 0, `MemAvailable` 24210812 kB —
  **old script: 3054 MiB, REFUSED, exit 1. New script: 10181 MiB, "ok, safe to
  start", exit 0.** So **yes, the honest preflight clears this host without an
  override**, at a reading where the old one refused it. No override was used
  at any point in this task.
- **`inactive_file` is a floor, and a moving one.** Under real pressure the
  kernel ages `active_file` down and reclaims it too, so the announced figure
  is below what the host would actually yield. That is the direction invariant
  2 requires, but it means a runner on a host with a hot, mostly-active cache
  still under-announces. If that ever matters, the measurement that would
  settle it is a real reclaim run, not more arithmetic.
- **The twelve skips mean the real-cgroup path is still unproven on this
  host.** Everything asserted here is asserted against fake-cgroup fixtures
  plus one read of this host's real `/sys/fs/cgroup` through `measure()`.

## Decisions needed

None blocking. One entry for `herd-merger` to allocate and append:

> **D-NEXT** (2026-10-29) — *A runner does not hold reclaimable page cache
> against the room it announces.* `herd-runner/src/room.rs` subtracted the
> enclosing cgroup's `memory.current` whole, and `memory.current` counts page
> cache, so a runner on a machine that had merely read files announced a
> fraction of its room — 3224 MiB under an 18 GiB ceiling on this project's
> host while `MemAvailable` said 22.3 GiB — and broke E-22's promise of
> `MemTotal` unless the owner says otherwise. The charge is now subtracted
> less its reclaimable file pages, and the reclaimable figure is
> **`inactive_file` less `shmem`**, not `file`: `inactive_file` is what the
> kernel drops first, `active_file` may be in use and stays charged, and
> `shmem` is swap-backed rather than droppable while still sitting on the file
> LRUs — and a runner's workspaces live on a tmpfs (D79). The figure is a
> floor on the room, never an estimate of it, because erring towards less room
> is the only direction invariant 2 allows. `scripts/bench-preflight.sh`
> carries the same arithmetic, as its own comment requires.

## Resume point

`git fetch && git log --oneline -1 origin/room-honest`, then take the branch
into batch 9's integration. Two follow-ups for whoever picks them up:

1. `herd-runner/src/main.rs:474-475`, the stale `Confined::prepare` comment
   named under *Not done*.
2. Run the suite once inside `systemd-run --user --scope -p Delegate=yes` to
   turn `a_tighter_enclosing_memory_max_lowers_the_room_that_is_announced`
   from a skip into a pass against a real cgroup.

## Processes

- Started and ended: `cargo`/`rustc`/`rustfmt`/`clippy` under
  `nix develop --command` in this worktree, and three `herd-runner`/`herd-hub`
  test binaries from `./scripts/test.sh`. All exited on their own; nothing was
  killed, no `pkill`, no kill by name. PID 3046930 (the stray `herd-hub` on
  `127.0.0.1:18461`) was not touched and no port of its was taken.
- `CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/room-honest` is deleted once the
  branch is pushed; the worktree's own `target/` was never created, since the
  build was directed outside it.
