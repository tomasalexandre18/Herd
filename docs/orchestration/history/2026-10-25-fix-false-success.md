# c2-false-success — handoff 2026-10-25

- **Item:** not on the board when this was written (campaign C2,
  remediation of `docs/remediation/04b-integrity-and-containment.md` §1)
- **Agent:** herd-step
- **Branch / commit:** `fix-false-success` / see below, based on
  origin/main `f507ab6`
- **PR:** none (herd-merger opens it)

## Measured, before and after

```
before (origin/main f507ab6)   herdctl exit code: 0, 1 manifest entry of 2
after  (this branch)           herdctl exit code: 2, 2 manifest entries
```

The "before" figure is this branch's own test run against the two
statements put back in their old order: it fails on the manifest
assertion, and with that one relaxed it fails on `left: 0, right: 2`.

## Done

**The cause is corrected in `herd-hub/src/store.rs`,
`JobDirStore::record_outputs`.** The manifest entry is now pushed *before*
the bytes are judged, so a frame whose bytes contradict their own sha256
loses the bytes and keeps the entry. The file is then announced and not
held — the shape the rest of the system already reads — and
`herdctl`'s shortfall count off the manifest sees it. Two statements
swapped; the refusal, the `skipped` sentence and the refusal to write the
bytes are all unchanged.

**The contract says both rules now**, on
`JobStore::record_outputs` in `herd-hub/src/job.rs`: that a refused digest
keeps its entry, and that a refused *path* does not, with the reason (the
manifest is the list of paths this store joins onto its own directory) and
with the explicit note that the exit code for the path case was never
decided.

**Non-regression tests, at two levels.**

* `herd-hub/src/store.rs`, unit:
  `bytes_that_disagree_with_their_digest_cost_the_bytes_and_not_the_entry`.
* `herdctl/tests/inline_digest_mismatch.rs`, end to end with the real
  `herdctl` binary as a child:
  `inline_bytes_that_contradict_their_digest_never_exit_zero` asserts the
  five criteria of 04b §4.1 plus the verdict document.
  Both benches of 04b that lived in this file were `measure_*` and only
  printed; they are expectations now.
* The neighbouring case is pinned as it stands by
  `a_path_the_hub_will_not_write_is_named_and_today_costs_no_exit_code`,
  whose comment says the 0 is inherited and undecided, so a change to it
  is deliberate.

**The stopgap, beside the fix and not instead of it.** `herdctl` now puts
every sentence about a job's files into `Verdict::said` as well as on
stderr: the hub's refusal, the `skipped` lines, and the per-path "not on
the hub" lines from the fetch. They do not move the exit code.

**`docs/decisions.md`** gained one entry headed `D-NEXT`, carrying the
defect, the rule it broke, why the correction is at the cause, the two
rejected alternatives, the position on the path case, and the impact
statement for the first consumer.

Neither `PROTOCOL_VERSION` nor `RECORD_VERSION` moves. No frame gains a
field. No record format changes.

## Not done, and why

* **The refused-path case is not changed.** The brief asked whether its 0
  was ever decided. It was not — see *Risks and findings* — and the brief
  then asked for a proposal, not an edit. Proposed in the decision entry
  and below.
* **`docs/status.md` and `docs/orchestration/board.md` are untouched.**
  The board is out of bounds; `status.md` is the orchestrator's state and
  another task is running beside this one.
* **`docs/coverage.md` is untouched.** No numbered expectation changes
  meaning; what changes is an exit code in one case, and the decision
  entry is where that is recorded.

## Gates

All four run under `nix develop`, with `CARGO_TARGET_DIR` off the root
disk, `-j 4`, and the host load read before each heavy phase (2.86, 3.74,
root disk 18% used).

```
cargo fmt --all --check      FMT OK
cargo clippy --workspace --all-targets -- -D warnings
                             Finished `dev` profile ... (no warning)
./scripts/test.sh            test suite: PASSED
                             12 assertion(s) COULD NOT RUN on this host
./scripts/check-docs.sh      docs check failed   (exit 1)
```

**The docs gate is red for exactly one reason and it is the required
one.** `scripts/check-docs.sh` refuses a `D-NEXT` placeholder:

```
docs/decisions.md still has a D-NEXT placeholder; herd-merger allocates
the number at integration:
8365:## 2026-10-25 — D-NEXT A refused file is named, never erased
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost)
docs check failed
```

The brief and `.claude/agents/herd-step.md` both require the entry to
carry `D-NEXT` until `herd-merger` allocates the number, so this gate
cannot be green on a step agent's branch. Checked by substituting a
number and re-running: `docs ok`, exit 0, with nothing else reported.
The placeholder was then put back.

**Skips: 12, which is the reference count** — 10 cgroup-v2 delegation
(this session's cgroup is root-owned and delegates nothing) and 2 ZFS
ceiling (the data root is on `rpool/root`, and no cgroup IO knob reaches
a ZFS dataset). Not counted as passes (D27).

## Risks and findings

* **F — the path case's exit code was never decided.** `grep` over
  `docs/decisions.md` finds nothing ruling on it. D131 settled the
  *record* (*"dropped and named on the record"*) and says of the
  over-the-cap refusal that it is *"a loud sentence and never an exit
  code"* — but that sentence is about a whole collection refused for size,
  not about one path the hub will not join, and D167 later overrode the
  general principle anyway by making a short collection cost the zero.
  D167 never mentions skipped or refused files. **So the 0 is inherited
  from the drop, exactly as the digest case's 0 was.**
* **F — but the two cases are not the same case, and the same fix does
  not fit.** `write_output_chunk` resolves an incoming chunk against the
  manifest, `job_outputs` reads a file back by it, the sweeper measures by
  it. Putting `../escape.json` on the manifest would put an unjoinable
  path in the one list this store joins. A fix for the path case has to
  carry the shortfall some other way, and that is a decision.
* **F, pre-existing, and now reachable by one more path.** A job holding a
  manifest entry whose bytes never landed never satisfies
  `collection_is_whole` (`herd-hub/src/store.rs`), and the output sweeper
  skips collections that are not whole (D152), so such a job's kept files
  are never evicted under the output budget. Already true of every
  streamed collection that never completed. Not fixed here: it is the
  sweeper's rule, not this defect's.
* **H — a streamed collection carrying inline bytes with a bad digest
  would now be held by the hub for up to `COLLECTION_SILENCE` (30 s) and
  released as `StoppedShort`, where before it was released at once with
  the file erased.** Only reachable from a runner that both streams and
  carries bytes inline on the same collection, which this build's runner
  does not do. What would settle it: a bench with `streamed: true` and one
  inline lying file. The behaviour is the right one — the hub asks for the
  bytes again — but the delay is new.

## Decisions needed

**Should a path the hub refuses also cost a requester its zero?**
Recommendation: **yes**, by the same reasoning as this correction — a
requester handed fewer files than the runner produced must not read 0 —
but carried by something other than a manifest entry, because an
unjoinable path must not be on the manifest. The cheapest shape is a count
of hub-side path refusals travelling beside the manifest and read by
`code_for`. Fallback: leave it, and the pinned test keeps it visible.
*Not* an acceptable shape: making any non-empty `skipped` exit non-zero —
that would turn D131's ordinary skips (a symlink, a fifo, a name that is
not UTF-8) into failures.

## Resume point

`herd-merger` allocates the `D-NEXT` number, re-runs
`scripts/check-docs.sh`, and opens the PR. Nothing is deployed.

## Processes

None left running. One backgrounded shell loop polling `/proc/loadavg`,
started and ended by this agent; `cargo` and `nix` under this agent's own
`CARGO_TARGET_DIR`, all exited.
