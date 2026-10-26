# orch-031-brief-worktree — handoff 2026-10-26

- **Item:** ORCH-031
- **Agent:** herd-step
- **Branch / commit:** orch-031-brief-worktree, based on origin/main c1d5ce7
- **PR:** #87

## Done

`docs/orchestration/templates/brief.md`. The worktree used to be a trailing
clause of the `Base:` line — `origin/main at <sha>; branch <name>; worktree
<path>` — which is how V11's brief came to leave it out. It is now its own
required field, absolute, with the creating command beside it, and
`Scratchpad:` is required and absolute next to it. `Forbidden:` names the
orchestrator's checkout explicitly. A paragraph under the code block says
why the field exists, cites V11, and states the one exemption.

`scripts/check-docs.sh`. A brief kept under `docs/orchestration/history/`
must name a worktree. The rule takes the absolute path written right after
the word `worktree`, and goes red when there is none — and red again when
that path is `/mnt/ssd_980/herd-project`. `git worktree add` does not
satisfy it: the path has to follow the word within four non-slash
characters, so a path that is merely an argument of the command does not
count as a named field. A brief whose agent writes nothing in the
repository says `read-only` on its `Base:` line instead.

Two scoping decisions worth naming, because both are places where a
looser rule would have been useless:

- **The exemption is matched on the `Base:` line only.** Nearly every
  brief ends `Write only: <paths>; everything else is read-only`, so an
  exemption that matched `read-only` anywhere in the file would have
  swallowed the entire rule. There is a red case for exactly this.
- **A brief is a file named `<task>-brief.<ext>`**, not any file whose
  name contains `brief`. I had the looser glob first and this handoff
  caught it: `2026-10-26-orch-031-brief-worktree.md` is a handoff, matched
  `*brief*`, and passed the rule only by accident, because a handoff about
  worktrees happens to mention a worktree path. A handoff that did not
  would have been failed for not being a brief. There is a green case for
  that too.

`scripts/check-orchestration.sh`: six cases, three red and three green, in
the existing "goes red on each defect it claims to catch" section beside
the undated-handoff case. CI's `docs` job runs this script next to
`check-docs.sh`, so they are real gates.

The five briefs already kept under `history/` that name a worktree pass
unchanged. The sixth, `v10-brief.txt`, names none: it was a read-only
release-preparation simulation whose agent wrote outside the repository,
and its `Base:` line says `read-only`, so it is exempt. **It was not
edited.** Those files are the verbatim record of what was sent to the live
`claude -p` sessions of 2026-10-24; adding a worktree line to one would
falsify the evidence that produced ORCH-031 in the first place. That is
why the exemption exists at all rather than my fixing the file.

## Not done, and why

- `docs/orchestration/board.md` was not touched, on instruction: three
  branches are in flight and the board is the file they would collide on.
  The row to apply is in "Resume point", ready to paste.
- `docs/orchestration/history/README.md` still says history holds
  handoffs, and does not mention that an archived brief is named
  `<task>-brief.<ext>` — the convention the rule keys on. I documented it
  in `templates/brief.md`, which is inside the paths this task was allowed
  to write. That README is the better home; it is a finding, not an edit.
- No `D-NEXT` entry: the brief did not ask for one, and nothing here
  changes a decision — it enforces one already recorded.
- Neither failing Rust test was touched. See below.

## Gates

**`./scripts/check-docs.sh` — green.**

```text
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
note: D174 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
exit=0
```

**`nix develop --command cargo clippy --workspace --all-targets -- -D
warnings` — green.**

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 13.15s
clippy exit=0
```

**`nix develop --command cargo fmt --all --check` — green**, no output,
exit 0.

**`nix develop --command ./scripts/test.sh` — RED.** Not from this change;
see "Risks and findings".

```text
thread 'a_killed_hub_reads_the_reservations_back_off_the_records' (2352910) panicked at herd-hub/tests/admission.rs:419:5:
the inherited job's 4 GiB of declared memory must still be held: [...] on lender, 2.5 GiB free of its 6.5 GiB memory admission, 5 GiB needed: [...]

failures:
    a_killed_hub_reads_the_reservations_back_off_the_records

test result: FAILED. 4 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 21.56s
error: test failed, to rerun pass `-p herd-hub --test admission`

Every assertion ran on this host: nothing was skipped.
test suite: FAILED (cargo test exited 101)
```

**Skips in that run: none reported, where the brief expected 12.** That is
a consequence of the failure, not a contradiction of the brief: `cargo
test` stops after the binary that fails, so `confinement` — the binary
carrying the cgroup skips — never ran and `$HERD_SKIP_LOG` stayed empty.
`test.sh` then printed "Every assertion ran on this host: nothing was
skipped", which in this situation is false and reassuring in the wrong
direction.

So I ran the suite again with `--no-fail-fast` to see all of it. **The 12
skips are there, exactly as the brief predicted** — 10 naming cgroup v2
delegation under
`/sys/fs/cgroup/chatons.slice/chatons-herd-claude-831000.scope`
(`Permission denied`), 2 naming the `io.max` ceiling and its device — and
there are **two** failures, not one:

```text
 12 assertion(s) COULD NOT RUN on this host and were skipped.
 A green run does NOT mean these were checked.

    a_killed_hub_reads_the_reservations_back_off_the_records   herd-hub/tests/admission.rs:419
    the_derived_j_is_recorded_and_is_what_the_job_is_given     herd-hub/tests/slots.rs:744

test suite: FAILED (cargo test exited 101)
exit=101
```

Everything else passed: 50 test binaries green.

**`./scripts/check-orchestration.sh` — green**, including the six new
cases. The brief did not name it; CI's `docs` job runs it, so it is
reported here.

```text
ok    a brief naming no worktree (the Write only boilerplate does not exempt it)
ok    a brief naming the orchestrator's checkout as its worktree
ok    a brief whose only path is inside a git worktree add command
ok    a brief naming an absolute worktree of its own
ok    a read-only brief, which writes nothing in the repository
ok    a handoff whose task name merely contains the word brief
```

**The rule red, then red, then green**, on a scratch copy of the tree —
the same brief three times, one line changing:

```text
--- the brief under test, with no worktree line:
Role: herd-step
Base: origin/main at c1d5ce7; branch demo
Write only: docs/x.md; everything else is read-only

--- run 1: ./scripts/check-docs.sh
docs/orchestration/history/2026-10-26-demo-brief.txt: a brief must name the agent's own worktree as an absolute path, or say read-only on its Base line (ORCH-031, V11)
docs check failed
exit=1

--- run 2: the same brief, worktree = the orchestrator's checkout
docs/orchestration/history/2026-10-26-demo-brief.txt: the worktree it names is /mnt/ssd_980/herd-project, the orchestrator's checkout; an agent works in its own worktree (ORCH-031, V11)
docs check failed
exit=1

--- run 3: the same brief, with a worktree of its own
Worktree:   /mnt/ssd_980/herd/claude/tmp/claude-1002/scratchpad/demo/wt
docs ok
exit=0
```

## Risks and findings

**Two Rust tests are deterministically red on this host and green in CI,
and neither can come from this branch.** `git diff --name-only origin/main
-- '*.rs' Cargo.toml Cargo.lock flake.nix` is empty: the Rust tree is
byte-identical to c1d5ce7. The `ci` run on c1d5ce7 itself concluded
`success` (`gh run list --branch main`). The admission one failed 3 times
out of 3 when run alone with nothing else on the machine, so it is not
load-flake like the two tests recorded in
`docs/remediation/00-baseline.md` §8 — and it is not either of those two.
That section says "whether any other test is unstable" is unknown and
needs repeated runs; these are those runs, and the answer is two more.

This was then settled from the other side: on PR #87 both CI jobs passed,
and CI's `check` job runs `nix develop --command ./scripts/test.sh` — the
very command that is red here (`.github/workflows/ci.yml`). The same
commit, the same command: green on `ubuntu-latest`, red on this machine.

Both failures are about a memory figure coming out smaller than the test
expects, which is also the theme of the 12 skips:

- `admission.rs:419`. The fixture `a_small_lender()` (`admission.rs:23`)
  declares `--host-memory 8GiB --host-memory-floor 1GiB`, and the test
  asserts the hub says `3 GiB free of its 7 GiB memory admission`, i.e.
  8 − 1. The hub says `2.5 GiB free of its 6.5 GiB memory admission` —
  1.5 GiB withheld where 1 was declared, identically in all four runs.
  `herd-runner/src/main.rs:588-599` resolves the floor, and
  `herd_proto::default_memory_floor_mib` supplies one when none is given.
- `slots.rs:744`. This assertion is already guarded by a host-capability
  branch, `herd_runner::metrics::total_memory_mib() >= 17 * 1024`
  (`slots.rs:742`). This host has 31.1 GiB total, so the branch is taken
  and `-j7` is required; the runner lent 6.1 GiB, derived `-j2`, and the
  assertion failed `left: "built with -j2"`, `right: "built with -j7"`.
  The guard keys on the host's *total* memory while the behaviour it
  asserts depends on the memory the runner actually *announces*. On this
  machine those two disagree, so the guard admits an assertion the host
  cannot satisfy.

I did not chase why the runner announces 6.1 GiB: it is product territory,
outside this task. For whoever does, two measurements that rule out the
easy explanations — the session's enclosing `chatons.slice` has
`memory.max = 23622320128` (22 GiB), and `MemAvailable` was 21.6 GiB
during the runs. Neither is near 6.1 GiB.

**I did not edit either test or either expectation**, and the red gate is
reported red.

**The rule is textual.** It trusts the word `worktree` and an absolute
path near it; it cannot know whether that directory exists, nor whether
the agent actually worked there. It closes the hole ORCH-031 names — a
brief that says nothing — and not the one where a brief names a worktree
and its agent ignores it. `orchestrator-guard.sh` cannot close that
either, because it exempts agents by design (`permissions.md`).

**The rule only sees briefs that are archived.** Briefs are normally
prompts, not files in the repository; `history/` holds handoffs, and the
only briefs in there are the six kept as H7 evidence. So this guards the
record, and the template guards the practice. A brief that is never
written down is still unchecked by anything.

**The checkout path is hardcoded** as `/mnt/ssd_980/herd-project`, the way
`.claude/agents/herd-step.md:14` already names it. On a CI runner the path
never appears and that half of the rule is inert — it is the half that
matters only on this machine, which is where the incident happened.

## Decisions needed

**Where do the two red tests go on the board?** Recommendation: one new
row for both, not an append to ORCH-022. ORCH-022 is a flaky test — green
on one commit and red minutes later on another — whereas these two are
deterministic here and green in CI, which is a different fault needing a
different fix; folding them in would lose the figures above. Fallback:
append them to ORCH-022 as further instances, keeping the 8 − 1.5 and the
`-j2`/`-j7` arithmetic verbatim.

**Not a decision, a small separate task:** `test.sh` printing "Every
assertion ran on this host: nothing was skipped" after `cargo test` bailed
out early is misleading in exactly the direction D27 exists to prevent —
the suite was red and most of it had not run. Making that sentence
conditional on a zero exit status, or on the binaries having all reported,
would be a few lines.

## Resume point

`herd-merger` merges the PR once CI is green — nobody else merges
(ORCH-001) — and applies the board row below, replacing the ORCH-031 row
where it stands in the **Owed** section. It does not need moving: ORCH-032
already sits there in state `done`.

```text
| ORCH-031 | A brief must name the agent's own worktree: the guard exempts agents by design, so nothing but the brief keeps one out of the orchestrator's checkout. V11's agent edited and staged `docs/status.md` there. | remediation H7 (V11), 2026-10-24 | herd-step | done | Done 2026-10-26: `templates/brief.md` makes `Worktree:` a required absolute field; `check-docs.sh` rejects a brief under `history/` that names no worktree or names the orchestrator's checkout; `check-orchestration.sh` runs six cases, three red and three green | — | `history/2026-10-26-orch-031-brief-worktree.md` |
```

## Processes

Five `nix develop --command cargo …` runs and four `./scripts/check-*.sh`
runs, each started and awaited by this agent inside its own worktree; all
exited on their own. No PID was killed, no `pkill`, no kill by name.
Nothing was written outside this task's own scratchpad subdirectory and
the worktree inside it:

```text
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/orch-031/
```
