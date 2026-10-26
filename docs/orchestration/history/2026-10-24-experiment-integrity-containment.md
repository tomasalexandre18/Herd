# experiment-integrity-containment — handoff 2026-10-24

- **Item:** ORCH-nnn (experiment 6.3 of the remediation dossier)
- **Agent:** herd-step
- **Branch / commit:** `experiment-integrity-containment`, based on
  `origin/main` `0a2cf73`
- **PR:** none — `herd-merger` opens it

## Done

Experiment 6.3 of the remediation dossier, in three parts, with no change
to any product file.

**(a) The false inline digest — confirmed.** A new bench,
`herdctl/tests/inline_digest_mismatch.rs`, joins a hand-written runner to a
real hub, runs the **real `herdctl` binary** as a child with
`--outputs-to`, and answers with a `JobFinished` carrying two inline files,
one of which contradicts its own sha256. Measured: **exit code 0**, one
manifest entry of two on `job.json`, one file written, and the skipped
sentence on stderr only. The audit's prediction and its mechanism are
confirmed line by line. A second case in the same file measures the
refused-path control (also 0, also 0 entries).

**(b) A runner flooded by a hub that is not one — confirmed.** A new bench,
`herd-hub/tests/fake_hub_flood.rs`, is a hand-written hub that hands a
real `herd_runner::serve_once` 50 and then 250 `AssignJob` frames. Jobs are
`sleep`, under a 256 MiB ceiling and `timeout_s = 25`. Measured: 50/50 and
**250/250 taken**, no refusal at either figure, the runner's own health
report saying 50 and 250, and the control link answering a probe in 360 µs
and 237 µs against a 2 000 ms budget. The load guard never bit; the
one-minute load did not move, because the jobs were `sleep`.

**(c) The proposal, on paper.** Six options for tying a verdict to the
commit really executed, with costs, protocol effect and migration path.
Recommended: a `#[serde(default)] ran_commit: Option<String>` on `StepRun`,
filled by the runner from a file the consumer's `fetch` hook writes,
compared by the hub and **recorded, not refused** — which moves neither
`PROTOCOL_VERSION` nor `RECORD_VERSION` and needs no migration, under the
additive rule already written at `docs/decisions.md:7293` and
`herd-hub/src/job.rs:44-51`. Nothing was implemented.

**The report** is `docs/remediation/04b-integrity-and-containment.md`, in
English, with non-regression and containment criteria for all three parts
and every claim carrying F, H or I.

## Not done, and why

- **No product code touched.** `herd-hub/src`, `herd-runner/src`,
  `herd-proto/src` and `herdctl/src` are byte-identical to `0a2cf73`. Only
  two test files and two documents were added.
- **No protocol field added.** (c) is a proposal; the field is Alexa's
  decision.
- **No flood of real jobs.** The host is shared with a production system,
  so (b) measured the *number* admitted and not the load a real workload
  would cause. That consequence is stated as **H** in the report.
- **No `herd-job` cgroup count.** This host delegates no cgroup v2 subtree
  to the session, so the figure is not a measurement; the bench says so in
  words and the workspace count stands in for it.
- **`docs/orchestration/board.md` untouched**, as the brief required.

## Gates

All four run through `nix develop --command`, with `-j 4` because the
machine is shared with a production system.

```
$ ./scripts/test.sh -j 4
test result: ok. 176 passed; 0 failed; … (45 binaries, every one ok)
==============================================================
 12 assertion(s) COULD NOT RUN on this host and were skipped.
 A green run does NOT mean these were checked.
==============================================================
test suite: PASSED

$ cargo clippy --workspace --all-targets -j 4 -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 10.60s

$ cargo fmt --all --check
(no output, exit 0)

$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost);
gaps stay (D148)
docs ok
```

**Skips: 12, which is the reference** — 10 cgroup-v2 delegation and 2
`io.max` on ZFS, named in full in the suite's own summary and none of them
read as a pass (D27). The two bench files add **no** skip: they assert
nothing, so there is no assertion to skip, and a `HERD-SKIPPED` from them
would have moved the reference for no reason. Where a figure could not be
measured — the `herd-job` cgroup count — the bench prints *"NOT A
MEASUREMENT"* in the same breath as the number, so the zero cannot be read
as one.

**One gate was red first, and is reported as red.**
`no_module_of_the_runner_can_bind_a_socket` failed on the first run because
the flood bench lived in `herd-runner/tests/` and named `TcpListener`. The
file moved to `herd-hub/tests/`; the rule was not touched. Finding 6.

## Risks and findings

1. **The brief's `store.rs:1711` is the `if`, not the `continue`.** The
   `continue` is at `herd-hub/src/store.rs:1715` and the comment stating the
   inverse rule begins at `:1717` — two lines under the `continue`, six
   under the `if`. The substance of the claim is exactly right.
2. **The skipped sentence is thinner evidence than the audit implies.** It
   reaches stderr and the hub's `job.json`, but **not** the verdict
   document: `said` is assembled at `herdctl/src/main.rs:713-723` and the
   skipped sentences never enter it. D166's machine-readable verdict is
   silent about a dropped output file. This makes the stopgap in §4.1 of the
   report (put them in `Verdict::said`) worth more than it would otherwise
   be, and it is a finding, not an edit.
3. **`PROTOCOL_VERSION` is 2 by a later decision, not by D73.** D73 froze
   both numbers at 1 from M16; D124/D128 then moved the protocol to 2 with
   Alexa's approval. The freeze *rule* is D73's; the current value is not.
4. **The freeze does not block (c).** The project already has a written
   additive rule and has applied it twice, and no protocol type carries
   `deny_unknown_fields`. The obstacle to an oracle for I01 is a decision
   about who produces the revision, not the version numbers.
5. **Invariant 2 is today the hub's property, not the runner's.** Confirmed
   at 250 jobs. Whether that is acceptable depends on a bound nobody has
   stated; audit B owns the arithmetic.
6. **The flood bench cannot live in `herd-runner`, and the gate said so.**
   `no_module_of_the_runner_can_bind_a_socket`
   (`herd-runner/tests/confinement.rs:1638-1668`) forbids `TcpListener`
   anywhere under that crate, tests included. The first run of
   `./scripts/test.sh` was **red** for exactly that reason; the file moved to
   `herd-hub/tests/fake_hub_flood.rs` and the rule was left alone. Worth
   knowing before anyone else writes a fake hub.

## Decisions needed

- **Does `StepRun` gain `ran_commit`?** Recommendation: yes, as Option 3 of
  §3.3 — additive, no version move, no migration, hub records a divergence
  rather than refusing it. Fallback if no: Option 2, ask the consumer to
  assert the commit in their own `fetch` hook, which costs Herd nothing and
  turns a wrong verdict into a setup failure. Impact of neither: I01 stays
  unfalsifiable.
- **Is the digest drop a defect to fix?** Recommendation: yes — invert
  `herd-hub/src/store.rs:1711-1730` so the manifest entry is pushed before
  the bytes are refused, which is what the function's own comment four lines
  below already prescribes. That is a product change and was not made here.

## Resume point

`herd-merger` takes the branch `experiment-integrity-containment`. The two
bench files are `measure_*` and assert nothing about their figures; if the
digest drop is fixed, `herdctl/tests/inline_digest_mismatch.rs` becomes the
expectation, with the criteria in §4.1 of the report as the assertions.

## Processes

`cargo`, `cargo test` and the child `herdctl` and `sleep` processes the
benches started, all under this worktree, all ended by the test harness or
by the runner's own cancellation path. Nothing was killed by name; no
`pkill`, no `killall`. No hub, runner, container or service in use was
touched.

## Host note

This worktree's `target/` reached 3.0 GB and the host's root filesystem
(`rpool/root`, 68 G) hit **100 % full** while the gates ran — another heavy
agent is building beside this one. `target/` was removed after the gates
passed, which gave 2.8 G back. Nothing outside this worktree was deleted.
Whoever runs the next heavy task on this host should check `df -h /` first.
