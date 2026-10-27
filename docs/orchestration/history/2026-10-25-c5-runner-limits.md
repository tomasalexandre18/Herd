# c5-runner-limits — handoff 2026-10-25

- **Item:** C5 — the runner holds its own limits
- **Agent:** herd-step
- **Branch / commit:** `runner-holds-its-own-limits` / `aa4699a`, based on
  origin/main `199e4df`
- **PR:** none (the brief forbids one)

## Done

- **A runner refuses past a ceiling it derives itself.**
  `herd_runner::jobs_at_once(host, memory_max)`
  (`herd-runner/src/confine.rs`) returns
  `max(1, (memory lent − floor kept) / per-job memory cap)` and the sentence
  that explains it. The floor is the host's own, or
  `herd_proto::default_memory_floor_mib` when it names none — the same
  default `herd-runner/src/main.rs` fills in, so a runner enforces the
  figure it announced. A host that will not say how much memory it lends
  holds **one** (`JOBS_AT_ONCE_WHEN_A_HOST_WILL_NOT_SAY`). At the shipped
  2 GiB a job: 12 on the 31 GiB server, 7 on the 17 GiB PC.
- **Held under the lock that writes the job down.** `Session::take_job` now
  returns `Result<CancelToken, NotTaken>`; `NotTaken` is `Draining`,
  `Switched(switch)` or `AtItsCeiling { running, most }` and the reason
  comes back from inside the lock, which also removes the old read of
  `session.draining()` *after* the refusal. What is counted is jobs with
  `end.is_none()`: a verdict waiting for an acknowledgement holds no process
  and no memory ceiling, so a slow hub cannot shut a working machine down.
- **A refusal is `ToHub::AttemptAbandoned` and nothing else.** No new
  variant anywhere; `PROTOCOL_VERSION` and `RECORD_VERSION` untouched;
  `JobEventKind` not approached. The hub requeues as it already did
  (`give_the_job_another_go`), and `herd-hub/tests/link.rs:2086-2113`
  already proves the job comes back out as attempt 2.
- **The `outputs/` leak is closed.** `reclaim_a_past_life` now sweeps
  `<data root>/outputs/` with the same `reclaim_directory` it uses for
  workspaces and names each spool with its size. Verified first that the
  leak was still real at the base: the only `collections.insert` is in
  `Session::finish`, nothing reads `outputs/` back, so a spool left by a
  killed runner could never be offered again.
- **The flood bench asserts.** `herd-hub/tests/fake_hub_flood.rs` declares
  its runner's host size (4 GiB lent, 2 GiB floor, 256 MiB a job → 8) so the
  figure is the same on every host, counts the `AttemptAbandoned` frames,
  and asserts the ceiling, the truthfulness of the health report and the
  workspace count. Its two tests lost their `measure_` prefix.
- **Tests added:** five on the derivation and one on the per-job-cap
  sentence (`herd-runner/src/confine.rs`), two on `take_job`
  (`herd-runner/src/lib.rs`), one on the spool reclaim
  (`herd-runner/src/reclaim.rs`), and the two reworked bench tests.
- **Docs:** `docs/remediation/09-runner-self-limits.md` (English) and a
  `D-NEXT` entry in `docs/decisions.md`.

### Figures, before and after

| | 50 before | 50 after | 250 before | 250 after |
|---|---|---|---|---|
| job workspaces (peak) | 50 | **8 (8)** | 250 | **8 (8)** |
| jobs on its health report | 50 | **8** | 250 | **8** |
| attempts handed back | 0 | **42** | 0 | **242** |
| control probe | 360 µs | 218 µs | 237 µs | 1.16 ms |

"Before" is audit 04b §2.2's measurement, not a re-run.

## Not done, and why

- **The runner does not announce its ceiling to the hub.** That is the fix
  for the over-placement risk below, and it is a `#[serde(default)]` field
  on `Hello` — additive, moving neither version number, but still protocol
  surface. The brief says to stop and say so. See *Decisions needed*.
- **Nothing on the hub was changed.** Its placement still has no per-runner
  job ceiling for jobs that declare no memory need.
- **`docs/remediation/README.md` does not list `09`.** It is another
  agent's piece and the brief forbids touching it.
- **The damage a real flood would do is still unmeasured.** The jobs are
  `sleep` by design; 250 compiling jobs is not a measurement to take on a
  shared host.

## Gates

```
cargo fmt --all --check                      exit 0
cargo clippy --workspace --all-targets -- -D warnings   exit 0
./scripts/test.sh                            test suite: PASSED
  12 assertion(s) COULD NOT RUN on this host and were skipped.
./scripts/check-docs.sh                      exit 1 — D-NEXT only
  docs/decisions.md still has a D-NEXT placeholder; herd-merger allocates
  the number at integration:
  8595:## 2026-10-25 — D-NEXT A runner holds its own ceiling
  note: D170 is a gap in docs/decisions.md …
  note: D174 is a gap in docs/decisions.md …
```

**Skips: 12, the reference figure** — 10 cgroup-v2 delegation, 2 ZFS
ceiling. Unchanged by this work, checked on every run.

**The red docs gate has exactly one cause**, proved rather than assumed:
`origin/main` at `199e4df` in a second worktree gives `docs ok`, exit 0, and
the only extra line on this branch is the `D-NEXT` one. The two `note:`
lines about gaps are `echo`s, not failures, and both are on the base too.
`./scripts/check-orchestration.sh` reports `86 passed, 1 failed`; the one
failure is its own `unmodified copy passes` case, which runs
`check-docs.sh` — the same single cause. The base scores `87 passed, 0
failed`.

## Risks and findings

- **F — the hub can over-place, and each refusal spends one of the job's
  goes.** `admission` (`herd-hub/src/matcher.rs:583-645`) charges a job only
  what it *declared*, so a job with no declared memory need costs a seat
  nothing and the hub will place any number of them on one runner. Every
  refusal goes through `give_the_job_another_go`, which counts against
  `attempts_allowed`, so a job refused enough times ends without a verdict.
  This is not new machinery — a drain has always cost a go the same way —
  but it is newly reachable. The clean fix is the announced ceiling under
  *Decisions needed*.
- **F — one test harness had to stop reading the host, and that is a
  finding, not a convenience.** `herd-hub/tests/support/mod.rs` built its
  runner from `Config::default()`, whose host size is the real machine, so
  with a ceiling `no_job_is_claimed_twice_when_passes_overlap` (twelve jobs
  at once, exactly one attempt each) would have had 12 places on this 32 GiB
  host and 6 on a 16 GiB CI box — the same test asserting two different
  things. It now declares 64 GiB lent with an 8 GiB floor. Reviewers should
  look at this specifically: it is the one place where a test's inputs were
  changed in the same commit as the behaviour.
- **H — a runner could ping-pong with a hub that keeps choosing it.** The
  hub re-dispatches a requeued job immediately and nothing stops it picking
  the same full runner. Bounded by `attempts_allowed`, so it is a few frames
  and not a hot loop, but it is wasted goes. What would settle it: a hub
  that knows the ceiling, or a hub that prefers a seat it has not just been
  refused by.
- **I — whether the consumer's fleet ever reaches a ceiling.** Nothing on
  their instance was read (D161).
- **Reverting** removes one check in `take_job` and the figure handed to it;
  nothing was written to disk, no record moved, no frame is new, so a fleet
  with one runner reverted and one not differs only in how many jobs each
  accepts. The `outputs/` sweep is the one half that is not undoable in
  effect — the bytes it removes are gone — but they were bytes no runner
  could ever have offered, and a revert simply stops the sweep.

## Decisions needed

**Should a runner announce its ceiling to the hub?** One
`#[serde(default)] jobs_at_once: Option<u32>` on `ToHub::Hello`, absent
meaning "did not say" and never "unlimited", with the hub counting a seat's
live attempts against it.

- *Recommendation:* **yes, next.** It is additive under the written rule at
  `docs/decisions.md:7293`, moves neither `PROTOCOL_VERSION` nor
  `RECORD_VERSION`, needs no migration, and it turns the refusal path from
  an ordinary occurrence back into the pathology it was meant for.
- *Fallback:* leave it. The runner is safe either way; the cost is jobs that
  spend their goes being refused by a machine the hub thought was free.

## Resume point

```sh
cd /tmp/claude-1002/-mnt-ssd-980-herd-project/\
3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/c5-runner-limits
export CARGO_TARGET_DIR=/mnt/ssd_980/herd/build/c5-runner-limits
./scripts/bench-preflight.sh && nix develop --command ./scripts/test.sh
```

For `herd-merger`: allocate the number for the `D-NEXT` entry at
`docs/decisions.md:8595` and add `09-runner-self-limits.md` to
`docs/remediation/README.md`'s table.

## Processes

None left. Every process this task started was a `cargo`, a `nix develop`
or a test child, each one waited for in the foreground; nothing was killed
by name and no `pkill` was used. The build tree
`/mnt/ssd_980/herd/build/c5-runner-limits` is removed at the end of the
task.
