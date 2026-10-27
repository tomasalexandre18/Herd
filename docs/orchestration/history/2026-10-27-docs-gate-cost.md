# docs-gate-cost — handoff 2026-10-27

- **Item:** ORCH-nnn (not allocated; ORCH-108 was already taken by the
  spool-deletion decision when this task began, so no id is written into
  the code or the board — `herd-merger` allocates it at integration)
- **Agent:** herd-step
- **Branch / commit:** `docs-gate-cost` / see below, based on origin/main
  `1e739fbc08b0bfe29a6c6f31d6df00e5b794b132`
- **PR:** none — one PR per batch since 2026-10-27; `herd-merger` batches
  this branch. **PR #100 is blocked on it**: its `docs` job is red for this
  reason alone, one handoff paragraph whose only sin is making the corpus
  bigger.

## Done

**The diagnosis held, and the cause is one line narrower than the brief
said.** Verified before changing anything:

- `check-docs.sh` on the corpus as it stands: **6009 ms**.
- `check-orchestration.sh`: **153.43 s**, of which the first **151.3 s** is
  the fixture section and the last **2.1 s** is everything else (both guard
  hooks, `test.sh`, `report-skips.sh`, `ops/agents`).
- Per-case gap between consecutive `ok` lines: **5.9 s**, flat across all
  25 cases — matching the brief's 11.3–11.8 s on the CI runner at about
  half this host's speed.
- The per-case cost is a whole-repository `check-docs.sh` run: **25**
  full-corpus runs (`expect_red` ×20, `expect_green` ×4, plus the
  gap-silence case) and **3** cheaper runs on the synthetic D1–D5 corpus,
  **28 invocations** in all. The `fresh()` copy itself is **5 ms** and is
  not the problem.

**What the brief did not know: 5.86 s of those 5.9 s was a single shell
loop.** The 79-column check selected lines with awk and then measured each
one in bash — one `read` per line, ~37 000 iterations per run — because awk
implementations disagree on `length` for UTF-8 and mawk, the awk on the CI
runner, counts bytes.

**So the fix is not to shrink the fixtures but to make the whole repository
cheap.** The width is now counted inside awk, in one pass per file, under
`LC_ALL=C` where every awk works on bytes:

```awk
cols = length(line) - gsub(/[\200-\277]/, "", line)
```

The character count of valid UTF-8 is its byte count minus its continuation
bytes (0x80–0xBF), because every encoded character contributes exactly one
leading byte. No locale support is assumed of awk, and no new dependency is
added.

**Why this choice over minimal fixture trees or a path argument.** Minimal
trees buy speed with exactly the coin the brief forbids spending: every
case still needs a `CLAUDE.md` with its pointers, a `status.md`, a
`coverage.md`, a board, five role files, hooks and `settings.json` just to
reach exit 0, so a "minimal" tree is a shrunken real one — and the cases
that depend on the real corpus (`known gaps D143, D154 not noted`, the
79-column case, `unmodified copy passes`) would become either vacuous or
untruthful. A path argument saves only the 5 ms copy. Making the script
fast keeps **all 25 cases running against the whole, real repository,
unchanged**, which is strictly stronger than any trimming, and it also
fixes the standalone `check-docs.sh` step in the same job.

**Measured after:**

| | before | after | factor |
|---|---|---|---|
| `check-docs.sh` | 6009 ms | **242 ms** | 24.8× |
| `check-orchestration.sh` | 153.43 s | **9.63 s** | 15.9× |
| the `docs` job (this host) | ~159 s | **~9.9 s** | 16× |
| the `docs` job (CI, ×1.9) | 5m12s (killed) | **~20 s** | — |

**Equivalence is proved, not assumed.** Old shell loop and new awk produce
**byte-identical findings**:

- On the corpus as it stands: 0 findings each, 6030 ms vs 80 ms for the
  width scan alone.
- On a corpus seeded with **150 real violations** across all 83 checked
  files, built from `a`, `—` (3 bytes), `≤` (3 bytes), `é` (2 bytes), a
  4-byte emoji, spaces and pipes, at lengths 70–94 straddling the limit:
  **150 findings each, same sha256 over the sorted sets**, line numbers and
  column counts included — under **gawk 5.4.1**, under **mawk 1.3.4** (the
  CI runner's awk, fetched via `nix shell nixpkgs#mawk`), and under the old
  bash loop. All three: `sha=bc9e22ee374c0fa4`.
- One deliberate divergence, in the strict direction: a line ending in a
  tab counted 85 before and 86 now, because bash's `read` stripped the
  trailing IFS character. No such line exists in the corpus.

**The suite got stronger, not weaker: 138 → 141 passed, 0 failed.** Three
cases were added, and the reason is a vacuity hole in the old suite. The
only width case was an **85-character ASCII** line, which is 85 bytes
either way: had the new awk counted bytes, the suite would have stayed
green on the exact property the rewrite turns on. The new cases put one on
each side of the limit with multibyte characters:

- a 79-character line of 3-byte characters (237 bytes) — green
- an 80-character line of 3-byte characters (240 bytes) — red
- a 79-character line of 4-byte characters (316 bytes) — green

**`timeout-minutes` was deliberately left at 5, and `.github/` is
untouched.** At ~20 s against a 300 s cap the headroom is ~280 s, and the
cap is the only thing that told us the cost was growing. Raising it would
spend the signal for nothing.

**Headroom in docs-lines, measured rather than extrapolated** — the corpus
grown with real handoff-shaped prose:

| docs lines | `check-docs.sh` |
|---|---|
| 36 730 | 232 ms |
| 44 190 | 301 ms |
| 59 110 | 356 ms |
| 88 950 | 577 ms |

Slope ≈ **6.6 ms per 1000 lines**, against ~161 ms per 1000 before: a 24×
flatter curve. On CI that is ~0.33 s per 1000 lines across the 26 runs, so
the 280 s of headroom is worth roughly **850 000 further docs-lines** —
about 23× the present corpus. Before this change the headroom was ~1000
lines, which is why one handoff tipped it over.

## Not done, and why

- **No board row written.** `herd-merger` owns `board.md`, the id was not
  allocated to me, and ORCH-108 — the number the next free one looked like
  when I started — is already the spool-deletion decision. Two agents
  allocating the same id is the collision `check-docs.sh` exists to catch;
  I did not create one. **Recommended row for the merger:** next free id,
  owner `herd-step` (done) / `herd-merger` (integration), state `done`,
  evidence this handoff plus the red runs below.
- **`check-docs.sh` is still linear in docs size** — now with a 24× smaller
  constant, which is enough for years at the present rate. Making it
  *sublinear* is a separate task and was deliberately not attempted here;
  see "Risks and findings" for the cheap shape it would take.
- **No `D-NEXT` entry.** The brief asked for none, and this is a cost fix
  inside an existing rule, not a new decision.

## Gates

Four Rust gates plus both docs scripts. The change touches only
`scripts/`, no Rust, so the two red targets below are pre-existing.

```
$ nix develop --command cargo build --workspace --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 07s
exit=0

$ nix develop --command cargo fmt --all --check
exit=0

$ nix develop --command cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 12.93s
exit=0

$ nix develop --command ./scripts/test.sh
test.sh exit=101
test suite: FAILED (cargo test exited 101)
  herd-hub/tests/admission.rs:419   a_killed_hub_reads_the_reservations_back_off_the_records
  herd-hub/tests/slots.rs:744       the_derived_j_is_recorded_and_is_what_the_job_is_given
  (every other target: 0 failed)

$ ./scripts/check-docs.sh
note: D170 is a gap in docs/decisions.md (held by an open PR, or lost); gaps stay (D148)
docs ok
exit=0, 242 ms

$ ./scripts/check-orchestration.sh
check-orchestration: 141 passed, 0 failed
exit=0, 9628 ms
```

**The suite is red, and the two red targets are the known ones**, named by
ORCH-069 and ORCH-107: `slots.rs:744` (constants derived from a 17 GiB
machine's live `MemTotal`; **skipped on CI**, and by D27 a skip is never a
pass — so "green on CI" is not claimed for it) and `admission.rs:419`,
whose failure here is ORCH-107's host contention. `git diff --stat` is two
shell scripts and nothing else, so neither can be this branch's.

**Skips: at least 12, and the list is a floor, not a count (D179)** — the
suite did not finish, so `test.sh` printed `AT LEAST 12 assertion(s) COULD
NOT RUN`. All twelve are the cgroup v2 delegation family (`io.max`,
`cpu.max`, `pids.max`, pressure, the enclosing `memory.max`), each needing
a delegated scope this host does not give. None is in the docs path.

## Risks and findings

- **The one real risk is the width count itself**, and it is the thing I
  measured hardest: byte-identical findings to the loop it replaces, over
  150 seeded violations, under both awks, plus three new cases that go red
  on a relapse to byte counting. The residual risk is an awk that is
  neither gawk nor mawk and mishandles an octal range in a bracket
  expression under `LC_ALL=C`; busybox awk was fetched but not exercised,
  and CI uses mawk, which was.
- **Proof the gate still bites** — four defects restored on a scratch copy
  outside the branch and outside this repository
  (`.../scratchpad/docs-gate/scratch`, its own `git init`, which reproduces
  141/0 before any breakage and returns to 141/0 after):

| defect restored | result |
|---|---|
| `cols = length(line)` — count bytes, the mawk relapse | **133 passed, 8 failed** |
| `cols > 7900` — the 79-column rule never fires | **139 passed, 2 failed** |
| fenced-code and table-row exemptions deleted | **133 passed, 8 failed** |
| ORCH-068's own defect: no `GH_CONFIG_DIR` in `env.sh` | **137 passed, 4 failed** |

  The byte-counting relapse is caught by both new multibyte cases **and**
  by `unmodified copy passes`, because the real corpus has wrapped lines
  holding `—` near the limit. Disabling the rule is caught by the old
  85-character case and the new 80-character one. The fourth run is a case
  this branch never touched, and it still bites.
- **The deeper cost, not fixed here and cheap when wanted.** What remains
  is ~242 ms, now dominated by the `seq 1 "$max"` gap scan and the greps
  over a 9412-line `docs/decisions.md`. The incremental shape is the
  obvious one: `docs/decisions.md` is append-only, so the fixture harness
  could check a **digest** of the files a case does not touch instead of
  re-reading them, or `check-docs.sh` could take a list of changed paths
  and check the corpus-wide rules only when a corpus file is among them.
  Neither is worth a branch today.
- **A caution for whoever grows the suite next.** The cost is
  *26 × the whole corpus* by construction, so any new rule in
  `check-docs.sh` that walks all the docs is paid 26 times. That multiplier
  is now the thing to watch, not the absolute number.

## Decisions needed

None. The one judgement call — fix the cost rather than trim the fixtures,
and leave `timeout-minutes` at 5 — is argued above and is internal, so by
CLAUDE.md it is not Alexa's.

## Resume point

`herd-merger`: batch `docs-gate-cost` into the next PR, allocate its board
id (next free, **not** ORCH-108) and write the row. The PR body wants the
before/after table and the four red runs from this handoff — the brief
asked for both to appear there. Nothing else is owed; the branch is
self-contained and touches no Rust.

## Processes

PIDs started: `cargo build`, `cargo clippy`, `cargo fmt`, one
`./scripts/test.sh`, and repeated `check-docs.sh` / `check-orchestration.sh`
runs, each in the foreground of my own shell and each exited on its own. No
background process survives this task; nothing was killed, and no `pkill`,
`killall` or kill-by-name was used.

Own `target/` deleted once the branch was pushed:

```
/mnt/ssd_980/herd/claude/tmp/claude-1002/-mnt-ssd-980-herd-project/3ddf11ac-718a-4231-bc8d-45291a39ef82/scratchpad/docs-gate/wt/target
```
